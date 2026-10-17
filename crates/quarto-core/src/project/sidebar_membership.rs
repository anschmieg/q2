/*
 * project/sidebar_membership.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pass-1 sidebar membership resolution.
//!
//! Returns the set of project documents each declared sidebar
//! contains, in declared order, with `auto:` directives expanded
//! against a [`ProjectIndex`]. Used by Phase 8's dependency graph
//! to derive sidebar co-membership edges.
//!
//! This is the **read-only**, side-effect-free counterpart to
//! Phase 2's [`SidebarGenerateTransform`](crate::transforms::sidebar_generate::SidebarGenerateTransform),
//! which produces resolved sidebars *with* enrichment, active-state
//! marking, and per-page picking. See
//! `claude-notes/designs/sidebar-auto-expansion-contract.md` for
//! the prose contract.
//!
//! The two helpers share the same underlying expansion code
//! ([`expand_auto`](crate::transforms::sidebar_auto::expand_auto)),
//! so the dependency-graph view of sidebar membership is consistent
//! with what the rendered sidebar actually displays.

use std::collections::HashSet;
use std::path::PathBuf;

use quarto_config::resolve_website_value;
use quarto_error_reporting::DiagnosticMessage;
use quarto_navigation::{Sidebar, SidebarEntry};
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceContext;

use crate::transforms::resolve_hrefs;

use crate::project::index::ProjectIndex;
use crate::transforms::sidebar_auto::expand_auto;

/// One resolved sidebar's membership set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSidebar {
    /// Sidebar id, when the user supplied `id:`. Multi-sidebar
    /// projects use this to disambiguate which sidebar applies to
    /// which page.
    pub id: Option<String>,

    /// Project-relative source paths in declared order. `auto:`
    /// directives are already expanded; section headers contribute
    /// their `href` if any. Pages declared as bare strings or via
    /// nested sections appear here exactly once each (dedup by
    /// path; first occurrence wins).
    pub members: Vec<PathBuf>,
}

/// Walk a project's sidebar configuration and return the membership
/// set per sidebar.
///
/// Reads the merged sidebar config (accepting both top-level
/// `sidebar:` and `website.sidebar:`; see
/// [`quarto_config::resolve_website_value`]) and produces a
/// [`ResolvedSidebar`] for each declared sidebar.
///
/// Diagnostics from `auto:` expansion are appended to
/// `diagnostics`. The function consumes none of `meta`'s ownership
/// and never mutates the index.
///
/// Returns an empty Vec if no sidebars are declared.
pub fn resolve_sidebar_membership(
    meta: &ConfigValue,
    index: &ProjectIndex,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<ResolvedSidebar> {
    resolve_sidebar_membership_inner(meta, index, diagnostics, None)
}

/// Live-preview variant: resolve authored hrefs against the declaring
/// document's SourceContext, exactly as SidebarGenerateTransform does.
/// The project-level graph builder continues to use its existing API.
pub fn resolve_sidebar_membership_for_render(
    meta: &ConfigValue,
    index: &ProjectIndex,
    diagnostics: &mut Vec<DiagnosticMessage>,
    source_context: &SourceContext,
    project_root: &std::path::Path,
) -> Vec<ResolvedSidebar> {
    resolve_sidebar_membership_inner(
        meta,
        index,
        diagnostics,
        Some((source_context, project_root)),
    )
}

fn resolve_sidebar_membership_inner(
    meta: &ConfigValue,
    index: &ProjectIndex,
    diagnostics: &mut Vec<DiagnosticMessage>,
    source: Option<(&SourceContext, &std::path::Path)>,
) -> Vec<ResolvedSidebar> {
    let Some(sidebar_cv) = resolve_website_value(meta, "sidebar") else {
        return Vec::new();
    };

    Sidebar::parse_list_from_config(&sidebar_cv)
        .into_iter()
        .map(|mut sidebar| {
            if let Some((context, root)) = source {
                resolve_hrefs(&mut sidebar.contents, context, root);
            }
            expand_auto(&mut sidebar, index, diagnostics);
            ResolvedSidebar {
                id: sidebar.id.clone(),
                members: collect_member_paths(&sidebar.contents),
            }
        })
        .collect()
}

/// Walk a sidebar's resolved entry tree and collect the project-
/// relative source path of every page that appears, in document
/// order. Skips entries that don't reference a page (separators,
/// pure-text headings, sections with no href).
fn collect_member_paths(entries: &[SidebarEntry]) -> Vec<PathBuf> {
    fn push_unique(out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, p: PathBuf) {
        if seen.insert(p.clone()) {
            out.push(p);
        }
    }

    fn walk(entries: &[SidebarEntry], out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
        for entry in entries {
            match entry {
                SidebarEntry::Link { item } => {
                    if let Some(href) = item.href.as_deref() {
                        // External URLs and fragment-only anchors
                        // aren't project pages; skip.
                        if !is_external_or_anchor(href) {
                            if let Some(source) = source_path_from_href(href) {
                                push_unique(out, seen, source);
                            }
                        }
                    }
                }
                SidebarEntry::Section { href, contents, .. } => {
                    if let Some(h) = href.as_deref()
                        && !is_external_or_anchor(h)
                    {
                        if let Some(source) = source_path_from_href(h) {
                            push_unique(out, seen, source);
                        }
                    }
                    walk(contents, out, seen);
                }
                SidebarEntry::Separator | SidebarEntry::Heading(_) | SidebarEntry::Auto(_) => {}
            }
        }
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    walk(entries, &mut out, &mut seen);
    out
}

/// Match the same document source key as navigation's HTML href resolver:
/// split the query/fragment and strip a site-root slash before indexing.
/// Do not silently normalize .. aliases — the WASM boundary must fail open
/// when a path cannot be expressed as a trustworthy sync identity.
fn source_path_from_href(href: &str) -> Option<PathBuf> {
    if is_external_or_anchor(href) {
        return None;
    }
    let path = href.split(['?', '#']).next().unwrap_or("");
    let path = path.strip_prefix('/').unwrap_or(path);
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Cheap classifier: a sidebar entry's `href` that *isn't* a
/// project page — any URI scheme or `//host` form (same rule as
/// [`crate::transforms::navigation_href::is_external`]), or a
/// fragment-only anchor.
fn is_external_or_anchor(href: &str) -> bool {
    quarto_util::is_external_url(href) || href.starts_with('#')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::for_test())
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::for_test())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn make_index_with(paths: &[&str]) -> ProjectIndex {
        let profiles: Vec<DocumentProfile> = paths
            .iter()
            .map(|p| DocumentProfile {
                source_path: PathBuf::from(p),
                output_href: p.replace(".qmd", ".html"),
                format_id: "html".to_string(),
                title: Some(crate::document_profile::text(p)),
                ..DocumentProfile::default()
            })
            .collect();
        ProjectIndex::new(profiles)
    }

    #[test]
    fn empty_meta_returns_no_sidebars() {
        let meta = ConfigValue::default();
        let index = make_index_with(&["a.qmd"]);
        let mut diags = Vec::new();
        assert!(resolve_sidebar_membership(&meta, &index, &mut diags).is_empty());
        assert!(diags.is_empty());
    }

    #[test]
    fn single_sidebar_string_entries() {
        // website.sidebar.contents: [a.qmd, b.qmd]
        let mut meta = ConfigValue::default();
        let sidebar = config_map(vec![("contents", arr(vec![s("a.qmd"), s("b.qmd")]))]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, None);
        assert_eq!(
            resolved[0].members,
            vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")]
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn repeated_nested_members_keep_first_appearance_order() {
        let nested = config_map(vec![
            ("section", s("Nested")),
            ("contents", arr(vec![s("a.qmd"), s("c.qmd"), s("a.qmd")])),
        ]);
        let sidebar = config_map(vec![(
            "contents",
            arr(vec![s("a.qmd"), nested, s("b.qmd"), s("a.qmd")]),
        )]);
        let mut meta = ConfigValue::default();
        meta.insert_path(&["website", "sidebar"], sidebar);
        let index = make_index_with(&["a.qmd", "b.qmd", "c.qmd"]);
        let mut diagnostics = Vec::new();
        let sidebars = resolve_sidebar_membership(&meta, &index, &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert_eq!(
            sidebars[0].members,
            vec![
                PathBuf::from("a.qmd"),
                PathBuf::from("c.qmd"),
                PathBuf::from("b.qmd"),
            ],
        );
    }

    #[test]
    fn sidebar_source_links_match_rendered_href_keys() {
        assert_eq!(
            source_path_from_href("/docs/about.qmd#intro?ref=sidebar"),
            Some(PathBuf::from("docs/about.qmd"))
        );
        assert_eq!(
            source_path_from_href("docs/about.qmd?ref=x#intro"),
            Some(PathBuf::from("docs/about.qmd"))
        );
        assert_eq!(source_path_from_href("#intro"), None);
        assert_eq!(source_path_from_href("https://example.com/a.qmd"), None);
        assert_eq!(source_path_from_href("//example.com/a.qmd"), None);
        assert_eq!(source_path_from_href("?ref=x"), None);
    }

    #[test]
    fn multi_sidebar_keeps_ids() {
        // website.sidebar: [{id: docs, contents: [a.qmd]}, {id: blog, contents: [b.qmd]}]
        let mut meta = ConfigValue::default();
        let docs = config_map(vec![("id", s("docs")), ("contents", arr(vec![s("a.qmd")]))]);
        let blog = config_map(vec![("id", s("blog")), ("contents", arr(vec![s("b.qmd")]))]);
        meta.insert_path(&["website", "sidebar"], arr(vec![docs, blog]));

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].id.as_deref(), Some("docs"));
        assert_eq!(resolved[0].members, vec![PathBuf::from("a.qmd")]);
        assert_eq!(resolved[1].id.as_deref(), Some("blog"));
        assert_eq!(resolved[1].members, vec![PathBuf::from("b.qmd")]);
    }

    #[test]
    fn auto_directive_expands_against_index() {
        // website.sidebar.contents: [auto: true]
        let mut meta = ConfigValue::default();
        let auto_entry = config_map(vec![(
            "auto",
            ConfigValue::new_bool(true, SourceInfo::for_test()),
        )]);
        let sidebar = config_map(vec![("contents", arr(vec![auto_entry]))]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd", "b.qmd", "c.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        // expand_auto enumerates non-draft pages from the index.
        // Order is index order (Pass-1 discovery).
        let names: Vec<&str> = resolved[0]
            .members
            .iter()
            .map(|p| p.to_str().unwrap())
            .collect();
        assert!(names.contains(&"a.qmd"));
        assert!(names.contains(&"b.qmd"));
        assert!(names.contains(&"c.qmd"));
    }

    #[test]
    fn nested_section_recurses() {
        // website.sidebar.contents:
        //   - section: "Group"
        //     contents: [a.qmd, b.qmd]
        let mut meta = ConfigValue::default();
        let section = config_map(vec![
            ("section", s("Group")),
            ("contents", arr(vec![s("a.qmd"), s("b.qmd")])),
        ]);
        let sidebar = config_map(vec![("contents", arr(vec![section]))]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].members,
            vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")]
        );
    }

    // ---- bd-jjep / bd-telo: accept top-level `sidebar:` form too ----

    #[test]
    fn top_level_sidebar_is_picked_up() {
        // Mirror image of the navbar/footer cliff: previously this
        // resolver only read `website.sidebar`, so top-level
        // `sidebar:` was silently ignored.
        let mut meta = ConfigValue::default();
        let sidebar = config_map(vec![("contents", arr(vec![s("a.qmd"), s("b.qmd")]))]);
        meta.insert_path(&["sidebar"], sidebar);

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].members,
            vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")]
        );
    }

    #[test]
    fn top_level_sidebar_concatenates_with_website_sidebar() {
        // Both locations present: the merged sidebar should pick up
        // entries from both layers. Arrays default to !concat, so the
        // resulting `contents` is the concatenation (nested first).
        let mut meta = ConfigValue::default();
        let nested = config_map(vec![("contents", arr(vec![s("a.qmd")]))]);
        let top = config_map(vec![("contents", arr(vec![s("b.qmd")]))]);
        meta.insert_path(&["website", "sidebar"], nested);
        meta.insert_path(&["sidebar"], top);

        let index = make_index_with(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved.len(), 1);
        // Both members appear; concat order is nested-then-top.
        assert_eq!(
            resolved[0].members,
            vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")]
        );
    }

    #[test]
    fn external_urls_excluded() {
        let mut meta = ConfigValue::default();
        let sidebar = config_map(vec![(
            "contents",
            arr(vec![s("a.qmd"), s("https://example.com")]),
        )]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        // External URL is dropped from membership; only project pages remain.
        assert_eq!(resolved[0].members, vec![PathBuf::from("a.qmd")]);
    }

    /// bd-scheme-href-path-normalized-w5zya82r — any scheme is external,
    /// not just the allowlisted few.
    #[test]
    fn any_scheme_href_excluded() {
        let mut meta = ConfigValue::default();
        let sidebar = config_map(vec![(
            "contents",
            arr(vec![
                s("a.qmd"),
                s("javascript:void(0);"),
                s("positron://settings/x"),
            ]),
        )]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved[0].members, vec![PathBuf::from("a.qmd")]);
    }

    #[test]
    fn duplicate_paths_deduped() {
        let mut meta = ConfigValue::default();
        let section = config_map(vec![
            ("section", s("Group")),
            ("contents", arr(vec![s("a.qmd"), s("a.qmd")])),
        ]);
        let sidebar = config_map(vec![("contents", arr(vec![s("a.qmd"), section]))]);
        meta.insert_path(&["website", "sidebar"], sidebar);

        let index = make_index_with(&["a.qmd"]);
        let mut diags = Vec::new();
        let resolved = resolve_sidebar_membership(&meta, &index, &mut diags);

        assert_eq!(resolved[0].members, vec![PathBuf::from("a.qmd")]);
    }
}
