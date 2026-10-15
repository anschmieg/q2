//! `GET /api/preview/deps?page=<rel>` endpoint (Phase D.6, bd-kw93.12).
//!
//! Returns the set of project-relative file paths that the page at
//! `<rel>` depends on — i.e. the files whose edits should cause the
//! active page to re-render. The SPA fetches this on boot and on
//! active-file change and on relevant edits, caches it locally, and uses it to filter
//! incoming `onFileContent` callbacks so unrelated sibling edits no
//! longer trigger a re-render.
//!
//! ## Project-mode include closure
//!
//! The HTTP endpoint runs Q2's own parse and include-expansion stages,
//! obtaining the *transitive* file closure from `DocumentAst::recorded_includes`.
//! The older `extract_include_deps` helper remains single-hop for its
//! syntax-unit tests; it is not used as a production substitute for the
//! renderer's nested-include semantics. Image and bibliography dependencies
//! still use the SPA's broad project-wide binary/config invalidation.
//!
//! The response distinguishes known-empty `{"deps":[]}` from UNKNOWN
//! `{"deps":null}`. Any parse, include, filesystem, symlink, or containment
//! uncertainty falls open: the SPA accepts every file edit rather than
//! suppressing the only event that could repair stale preview output.
//! Since this native collector reads disk, it is disabled for shared and
//! editable sessions whose VFS may be ahead of the disk source.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quarto_core::stage::stages::collect_include_paths;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::stage::{
    LoadedSource, ParseDocumentStage, PipelineData, PipelineStage, StageContext,
    expand_document_includes,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_hub::context::SharedContext;
use serde::{Deserialize, Serialize};

use crate::diagnostics;

#[derive(Debug, Deserialize)]
pub struct DepsQuery {
    pub page: String,
}

#[derive(Debug, Serialize)]
pub struct DepsResponse {
    /// Transitive project-relative include paths. `None` means that the
    /// analysis could not prove a complete set: clients MUST render on
    /// every file change (fail-open) rather than treat this as empty.
    pub deps: Option<Vec<String>>,
}

/// Axum handler for `GET /api/preview/deps?page=<rel>`.
pub async fn deps_handler(
    State(ctx): State<SharedContext>,
    Query(query): Query<DepsQuery>,
) -> Response {
    let rel_path = query.page;

    // Path validation: must be tracked in the project index. Files
    // outside the project (or unknown paths) → 400.
    if !ctx.index().has_file(&rel_path) {
        return (
            StatusCode::BAD_REQUEST,
            format!("Path '{rel_path}' is not in the project index"),
        )
            .into_response();
    }

    let Some(project_root) = ctx.storage().project_root().map(|p| p.to_path_buf()) else {
        // Standalone mode (no project) — dependency set is unknown.
        return Json(DepsResponse { deps: None }).into_response();
    };
    let project_root = project_root.canonicalize().unwrap_or(project_root);

    let abs_path = project_root.join(&rel_path);
    let source = match std::fs::read(&abs_path) {
        Ok(s) => s,
        Err(e) => {
            // bd-b9kzg: surface IO failures via the per-page sink
            // in addition to (instead of) the existing tracing
            // line. `emit` calls `tracing::warn!` itself, so this
            // remains a clean replacement when the sink is set,
            // and falls back to the original log line when it
            // isn't (e.g. unit tests that call the handler
            // without booting a server).
            if let Some(sink) = diagnostics::current_sink() {
                sink.emit(
                    &rel_path,
                    DiagnosticMessageBuilder::warning("Could not analyze includes")
                        .with_code("Q-PREVIEW-DEPS-1")
                        .problem(format!("Failed to read source for dep extraction: {e}"))
                        .build(),
                );
            } else {
                tracing::warn!(
                    rel_path = %rel_path,
                    error = %e,
                    "could not read source for dep extraction; falling back to broad invalidation",
                );
            }
            return Json(DepsResponse { deps: None }).into_response();
        }
    };

    // The native resolver reads the project filesystem. In writable or
    // shared sessions the synchronized VFS can lead the disk; a disk-based
    // dependency list must not be used to filter live editor changes.
    if super::ALLOW_EDIT.get().copied().unwrap_or(false)
        || super::SHARED_PREVIEW.get().copied().unwrap_or(false)
    {
        return Json(DepsResponse { deps: None }).into_response();
    }

    // The real parser and expansion can be CPU-intensive on large include
    // trees; keep them off the async HTTP runtime's worker threads.
    let root = project_root.clone();
    let page = rel_path.clone();
    let deps = tokio::task::spawn_blocking(move || {
        resolve_project_include_deps(&root, &page, source)
    })
    .await
    .unwrap_or(None);
    if deps.is_none() {
        tracing::warn!(
            page = %rel_path,
            "incomplete project include closure; preview dependency filter falls open"
        );
    }
    Json(DepsResponse { deps }).into_response()
}

/// Conservative alias check using the existing syntax walker, NOT a second
/// include parser. The native renderer records canonical targets, while hub
/// file-change notifications are keyed by source-relative paths. Until the
/// hub provides an alias map, an include through a symlink must fail open.
/// This check also rejects missing and out-of-project include paths.
fn include_keys_are_canonical(root: &Path, page_rel: &str, content: &[u8]) -> bool {
    extract_include_deps(content, page_rel).into_iter().all(|rel| {
        let lexical = root.join(rel);
        lexical.canonicalize().is_ok_and(|resolved| resolved == lexical)
    })
}

/// Compute the full project-mode include closure using the same parser and
/// IncludeExpansionStage as the native renderer (including nested containers,
/// includes within code fences, and recursive includes).
///
/// `None` is not a valid empty set: it means that dependency analysis cannot
/// certify the closure, so the SPA must accept ALL incoming source edits.
/// In particular, missing/cyclic includes and paths escaping the project are
/// *unknown*, never falsely considered to have zero dependencies.
///
/// This disk snapshot is only authoritative for non-editable, non-shared
/// preview sessions; the HTTP handler disables selective filtering otherwise.
fn resolve_project_include_deps(
    project_root: &Path,
    page_rel: &str,
    source: Vec<u8>,
) -> Option<Vec<String>> {
    let root = project_root.canonicalize().ok()?;
    let requested = root.join(page_rel);
    let abs_page = requested.canonicalize().ok()?;
    if !abs_page.starts_with(&root) || !abs_page.is_file()
        || abs_page != requested
    {
        return None;
    }

    // The request's source and the expansion's child reads must represent a
    // stable disk snapshot. An overlapping write degrades to unknown.
    let source_snapshot = source.clone();
    if std::fs::read(&requested).ok()? != source_snapshot
        || !include_keys_are_canonical(&root, page_rel, &source_snapshot)
    {
        return None;
    }

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    // Discover from the requested file, not the project directory.
    // This loads the same project config and root while avoiding a recursive
    // project-wide file walk on every dependency lookup. The include expander
    // only needs the project root and reads the include bytes itself.
    let project = ProjectContext::discover(&abs_page, runtime.as_ref()).ok()?;
    let document = DocumentInfo::from_path(&abs_page);
    let mut ctx = StageContext::new(runtime, quarto_core::Format::html(), project, document).ok()?;

    let parse = ParseDocumentStage;
    let PipelineData::DocumentAst(mut doc) = pollster::block_on(parse.run(
        PipelineData::LoadedSource(LoadedSource::new(abs_page, source)),
        &mut ctx,
    )).ok()? else {
        return None;
    };

    // The expander populates recorded_includes with the exact transitive
    // includes the renderer consumed; a failure is conservatively unknown,
    // even if a partial record exists (missing file, cycle, parse failure).
    expand_document_includes(&mut doc, &mut ctx).ok()?;
    let mut deps = Vec::new();
    for entry in &doc.recorded_includes {
        let canonical = entry.path.canonicalize().ok()?;
        if !canonical.starts_with(&root) || !canonical.is_file() {
            return None;
        }
        // Symlink aliases can have a different VFS key from their canonical
        // target. Until the hub exposes an authoritative alias mapping,
        // don't silently miss an edit to such a path.
        if canonical != entry.path {
            return None;
        }
        // Check for concurrent child updates after expansion. The renderer
        // recorded the content hash of the bytes it actually consumed.
        let current = std::fs::read(&canonical).ok()?;
        if quarto_core::document_profile::IncludeEntry::hash_bytes(&current)
            != entry.content_hash
        {
            return None;
        }
        let rel = canonical.strip_prefix(&root).ok()?;
        let relative = normalize_forward_slash(rel);
        if !include_keys_are_canonical(&root, &relative, &current) {
            return None;
        }
        deps.push(relative);
    }
    if std::fs::read(&requested).ok()? != source_snapshot {
        return None;
    }
    deps.sort();
    deps.dedup();
    Some(deps)
}

/// Extract include-shortcode dependencies from a qmd source.
///
/// Parses `source` into a Pandoc AST via `pampa::readers::qmd::read`,
/// then walks every block-list position for the same "is this an
/// include shortcode?" shape [`IncludeExpansionStage`] uses (via the
/// shared [`collect_include_paths`] walker, which mirrors the
/// expander's traversal exactly). That includes the *text* of code
/// fences, where a lone `{{< include … >}}` line embeds a source file
/// as a listing (bd-include-in-code-block-f8mvtczn) — those targets are
/// dependencies too. Paths are resolved relative to `page_rel`'s
/// directory and emitted as forward-slash project-relative strings,
/// deduplicated and sorted.
///
/// The legacy syntax-only extractor returns an empty list on parse failure;
/// production does not use it as a complete dependency set.
///
/// Public so unit tests + a potential future debugging surface can
/// reach it without spinning up an axum router.
pub fn extract_include_deps(source: &[u8], page_rel: &str) -> Vec<String> {
    // Pampa's reader writes commentary to an output stream we don't
    // care about here; sink it. Source-location tracking is off
    // (`false` for the `track_source_locations` argument) since we
    // don't need spans for include-name extraction.
    let mut sink = std::io::sink();
    let parse_result = pampa::readers::qmd::read(
        source, false,    // loose
        page_rel, // filename, for any error messages
        &mut sink, false, // track_source_locations
        None,  // parent SourceInfo
    );

    let Ok((mut pandoc, _ast_context, _warnings)) = parse_result else {
        // bd-b9kzg: parse failures surface to the SPA via the
        // per-page sink in addition to the existing tracing line.
        // Pampa's parser is robust enough that this branch is
        // rarely hit in practice (the unit test
        // `pathological_input_does_not_panic` covers garbage
        // input), but when it IS hit, the user benefits from
        // seeing "include analysis failed" in the overlay rather
        // than the silent empty-deps fallback. As elsewhere,
        // `emit` calls `tracing::warn!` itself, so this is a
        // clean replacement when the sink is set.
        if let Some(diag_sink) = diagnostics::current_sink() {
            diag_sink.emit(
                page_rel,
                DiagnosticMessageBuilder::warning("Could not analyze includes")
                    .with_code("Q-PREVIEW-DEPS-2")
                    .problem(
                        "Document failed to parse during include extraction; \
                         dep-graph filter will fall back to broadcasting edits.",
                    )
                    .build(),
            );
        } else {
            tracing::warn!(
                page_rel = %page_rel,
                "pampa parse failed during dep extraction; returning no deps",
            );
        }
        return Vec::new();
    };

    let page_dir = Path::new(page_rel)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    // `collect_include_paths` takes `&mut` so it can share the
    // expander's container-position accessor (see its docs); we own
    // this freshly-parsed AST, so that costs nothing.
    let mut deps: Vec<String> = collect_include_paths(&mut pandoc.blocks)
        .into_iter()
        .map(|raw| {
            // Resolve to a forward-slash project-relative path,
            // mirroring `IncludeExpansionStage`'s anchors
            // (bd-w9koo1i2): a leading `/` (or `\`) is
            // project-root-relative — dep strings are already
            // project-relative, so the root anchor means dropping the
            // slash, NOT prepending the page dir. Anything else is
            // page-relative. We don't canonicalize against the
            // filesystem — the SPA matches these strings against
            // paths from `onFileContent`, which arrive in the same
            // forward-slash form.
            let joined = if raw.starts_with('/') || raw.starts_with('\\') {
                PathBuf::from(raw.replace('\\', "/").trim_start_matches('/'))
            } else {
                page_dir.join(&raw)
            };
            normalize_forward_slash(&joined)
        })
        .collect();
    deps.sort();
    deps.dedup();
    deps
}

fn normalize_forward_slash(p: &Path) -> String {
    // Use components() to collapse `./` and resolve `..` against
    // earlier components. Doesn't touch the filesystem.
    let mut parts: Vec<String> = Vec::new();
    for c in p.components() {
        use std::path::Component;
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir => {
                // Absolute path — shouldn't happen for include
                // references; fall back to the raw form so callers
                // notice the surprise.
                return p.to_string_lossy().replace('\\', "/");
            }
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps_for(src: &str, page: &str) -> Vec<String> {
        extract_include_deps(src.as_bytes(), page)
    }

    #[test]
    fn returns_empty_for_no_includes() {
        let src = "# Hello\n\nNo shortcodes here.\n";
        assert!(deps_for(src, "index.qmd").is_empty());
    }

    #[test]
    fn unquoted_include() {
        let src = "# A\n\n{{< include x.qmd >}}\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["x.qmd"]);
    }

    #[test]
    fn double_quoted_include() {
        let src = "# A\n\n{{< include \"posts/intro.qmd\" >}}\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["posts/intro.qmd"]);
    }

    #[test]
    fn single_quoted_include() {
        let src = "# A\n\n{{< include 'data/setup.qmd' >}}\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["data/setup.qmd"]);
    }

    #[test]
    fn include_resolves_relative_to_page_dir() {
        // page = posts/post1.qmd; include = setup.qmd → posts/setup.qmd
        let src = "# Post\n\n{{< include setup.qmd >}}\n";
        assert_eq!(deps_for(src, "posts/post1.qmd"), vec!["posts/setup.qmd"]);
    }

    #[test]
    fn include_resolves_parent_dir() {
        // page = posts/post1.qmd; include = ../shared.qmd → shared.qmd
        let src = "# Post\n\n{{< include ../shared.qmd >}}\n";
        assert_eq!(deps_for(src, "posts/post1.qmd"), vec!["shared.qmd"]);
    }

    #[test]
    fn project_absolute_include_is_project_relative_dep() {
        // bd-w9koo1i2: a leading `/` is project-root-relative (Quarto
        // path convention), and dep strings are project-relative
        // forward-slash paths — so the leading slash strips and the
        // page directory is NOT prepended.
        let src = "# Post\n\n{{< include /sub/_includes/x.qmd >}}\n";
        assert_eq!(
            deps_for(src, "sub/doc.qmd"),
            vec!["sub/_includes/x.qmd"],
            "leading-/ include must anchor at the project root, not the page dir"
        );
    }

    #[test]
    fn multiple_includes_deduped_and_sorted() {
        let src = "# Mix\n\n{{< include z.qmd >}}\n\n{{< include a.qmd >}}\n\n{{< include z.qmd >}}\n\n{{< include \"m.qmd\" >}}\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["a.qmd", "m.qmd", "z.qmd"]);
    }

    #[test]
    fn unrelated_shortcodes_are_ignored() {
        // Non-`include` shortcodes don't contribute deps. The AST
        // walker filters by `shortcode.name == "include"`, so
        // `{{< meta title >}}` etc. don't show up.
        let src = "# Other\n\n{{< meta title >}}\n";
        assert!(deps_for(src, "index.qmd").is_empty());
    }

    #[test]
    fn inline_include_mixed_with_text_does_not_count() {
        // `IncludeExpansionStage`'s rule: an include shortcode must
        // be the *only* inline in its paragraph. Mixed-with-text
        // doesn't qualify as a true include. This is the
        // canonical Quarto-renderer behaviour — pinning it here
        // keeps the dep filter in lock-step with the renderer.
        let src = "# A\n\nSome prose then {{< include foo.qmd >}} inline.\n";
        assert!(
            deps_for(src, "index.qmd").is_empty(),
            "an inline include doesn't expand at render time, so it isn't a dep"
        );
    }

    #[test]
    fn includes_nested_in_containers_are_deps() {
        // bd-1fz3vh99: the renderer expands includes at every
        // block-list position (divs, blockquotes, list items), so the
        // dep filter must see them too — otherwise nested includes
        // mean stale previews, the exact bug class this module
        // exists to prevent.
        let src = "# A\n\n\
            ::: {.callout-note}\n{{< include in-div.qmd >}}\n:::\n\n\
            > {{< include in-quote.qmd >}}\n\n\
            - {{< include in-list.qmd >}}\n- other\n";
        assert_eq!(
            deps_for(src, "index.qmd"),
            vec!["in-div.qmd", "in-list.qmd", "in-quote.qmd"]
        );
    }

    #[test]
    fn include_inside_code_block_is_a_dep() {
        // bd-include-in-code-block-f8mvtczn: a lone include inside a
        // fence splices the target's text into the listing (the Q1
        // idiom for embedding a source file), so the embedded file is
        // a real dependency — editing it must rebuild the page that
        // shows it. This reverses the earlier contract, when a fence
        // include was inert text.
        let src = "# A\n\n```{.python}\n{{< include app.py >}}\n```\n";
        assert_eq!(deps_for(src, "index.qmd"), vec!["app.py"]);
    }

    #[test]
    fn include_inside_opted_out_code_block_is_not_a_dep() {
        // `shortcodes="false"` means the fence displays the syntax
        // rather than expanding it, so there is nothing to depend on.
        let src = "# A\n\n```{.markdown shortcodes=\"false\"}\n{{< include app.py >}}\n```\n";
        assert!(deps_for(src, "index.qmd").is_empty());
    }

    #[test]
    fn mid_line_include_inside_code_block_is_not_a_dep() {
        // Recognition is line-strict (Q1's `isBlockShortcode`), so a
        // shortcode sharing its line with code is inert text and
        // contributes no dependency.
        let src = "# A\n\n```{.python}\nx = 1  {{< include app.py >}}\n```\n";
        assert!(deps_for(src, "index.qmd").is_empty());
    }

    #[test]
    fn project_mode_tracks_transitive_includes_and_filters_unrelated_files() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        std::fs::write(root.join("_quarto.yml"), "project:\n  type: website\n").unwrap();
        std::fs::create_dir(root.join("snippets")).unwrap();
        std::fs::write(root.join("index.qmd"), "# Page\n\n{{< include snippets/outer.qmd >}}\n").unwrap();
        std::fs::write(root.join("snippets/outer.qmd"), "{{< include leaf.qmd >}}\n").unwrap();
        std::fs::write(root.join("snippets/leaf.qmd"), "Current leaf\n").unwrap();
        std::fs::write(root.join("unrelated.qmd"), "# Sibling\n").unwrap();
        let source = std::fs::read(root.join("index.qmd")).unwrap();
        let closure = resolve_project_include_deps(root, "index.qmd", source).unwrap();
        assert_eq!(closure, vec!["snippets/leaf.qmd", "snippets/outer.qmd"]);
        assert!(!closure.contains(&"unrelated.qmd".to_string()));
    }

    #[test]
    fn project_mode_updates_closure_after_additions_and_removals() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        std::fs::write(root.join("_quarto.yml"), "project:\n  type: website\n").unwrap();
        std::fs::write(root.join("index.qmd"), "{{< include outer.qmd >}}\n").unwrap();
        std::fs::write(root.join("outer.qmd"), "Hello\n").unwrap();
        let get = || resolve_project_include_deps(root, "index.qmd", std::fs::read(root.join("index.qmd")).unwrap());
        assert_eq!(get().unwrap(), vec!["outer.qmd"]);
        std::fs::write(root.join("extra.qmd"), "Hello new dep\n").unwrap();
        std::fs::write(root.join("outer.qmd"), "{{< include extra.qmd >}}\n").unwrap();
        assert_eq!(get().unwrap(), vec!["extra.qmd", "outer.qmd"]);
        std::fs::write(root.join("outer.qmd"), "No longer includes extra\n").unwrap();
        assert_eq!(get().unwrap(), vec!["outer.qmd"]);
    }

    #[test]
    fn project_mode_fails_open_for_missing_and_cyclic_includes() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        std::fs::write(root.join("_quarto.yml"), "project:\n  type: website\n").unwrap();
        std::fs::write(root.join("index.qmd"), "{{< include outer.qmd >}}\n").unwrap();
        std::fs::write(root.join("outer.qmd"), "{{< include missing.qmd >}}\n").unwrap();
        let get = || resolve_project_include_deps(root, "index.qmd", std::fs::read(root.join("index.qmd")).unwrap());
        assert_eq!(get(), None, "missing child must not yield incomplete known deps");
        std::fs::write(root.join("missing.qmd"), "recreated\n").unwrap();
        assert_eq!(get().unwrap(), vec!["missing.qmd", "outer.qmd"]);
        std::fs::write(root.join("outer.qmd"), "{{< include index.qmd >}}\n").unwrap();
        assert_eq!(get(), None, "cycles must conservatively disable filtering");
    }

    #[cfg(unix)]
    #[test]
    fn project_mode_fails_open_for_outside_root_symlink() {
        let fixture = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = fixture.path();
        std::fs::write(root.join("_quarto.yml"), "project:\n  type: website\n").unwrap();
        std::fs::write(root.join("index.qmd"), "{{< include escape.qmd >}}\n").unwrap();
        std::fs::write(outside.path().join("secret.qmd"), "outside\n").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.qmd"), root.join("escape.qmd")).unwrap();
        let closure = resolve_project_include_deps(root, "index.qmd", std::fs::read(root.join("index.qmd")).unwrap());
        assert!(closure.is_none(), "outside-root dependency must never be returned");
    }

    #[cfg(unix)]
    #[test]
    fn project_mode_fails_open_for_in_root_symlink_alias() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        std::fs::write(root.join("_quarto.yml"), "project:\n  type: website\n").unwrap();
        std::fs::write(root.join("index.qmd"), "{{< include alias.qmd >}}\n").unwrap();
        std::fs::write(root.join("leaf.qmd"), "target\n").unwrap();
        std::os::unix::fs::symlink(root.join("leaf.qmd"), root.join("alias.qmd")).unwrap();
        let closure = resolve_project_include_deps(
            root, "index.qmd", std::fs::read(root.join("index.qmd")).unwrap(),
        );
        assert_eq!(closure, None, "VFS alias and canonical path keys could diverge");
    }

    #[test]
    fn single_hop_is_documented_limitation() {
        // a.qmd directly includes b.qmd. The extractor does NOT
        // recursively open b.qmd to look at its includes; single-hop
        // is the v1 contract. Future transitive expansion would be
        // a deliberate behaviour change.
        let src = "# A\n\n{{< include b.qmd >}}\n";
        assert_eq!(deps_for(src, "a.qmd"), vec!["b.qmd"]);
    }

    #[test]
    fn pathological_input_does_not_panic() {
        // The pampa parser is robust enough to handle invalid UTF-8
        // and arbitrary byte garbage without panicking, but the
        // contract this test pins is "never panics, never returns
        // an Err to the caller, always returns a Vec." If a future
        // pampa change makes some input genuinely return Err, the
        // fail-open `Ok(_) else return Vec::new()` branch keeps
        // this contract intact.
        let garbage = b"\xff\xfe\xfd not valid utf-8 at all";
        let _result: Vec<String> = extract_include_deps(garbage, "index.qmd");
        // No assertion needed beyond "the call returned." The type
        // system guarantees a Vec<String>; the contract is no panic.
    }

    #[test]
    fn normalize_forward_slash_round_trips() {
        assert_eq!(
            normalize_forward_slash(Path::new("posts/intro.qmd")),
            "posts/intro.qmd"
        );
        assert_eq!(
            normalize_forward_slash(Path::new("a/b/../c.qmd")),
            "a/c.qmd"
        );
        assert_eq!(normalize_forward_slash(Path::new("./a.qmd")), "a.qmd");
    }
}
