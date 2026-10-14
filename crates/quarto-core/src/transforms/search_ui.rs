/*
 * transforms/search_ui.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Per-page wiring for the website search UI (bd-eip0naf4).
 */

//! Ship the search UI with every HTML page of a project that has
//! search enabled: the options JSON the runtime reads, the
//! `quarto:offset` meta it resolves site-root-relative URLs with, and
//! the scripts themselves.
//!
//! Stores `rendered.search.head` (the `<meta>` + `<script
//! id="quarto-search-options">` block the template puts in `<head>`;
//! the template also emits the `#quarto-search-results` panel container
//! when this key is set) and registers two Project-scoped artifacts,
//! `js:search:autocomplete.umd.js` (Algolia Autocomplete 1.19.1, the
//! widget framework — see `resources/search/README.md`) and
//! `js:search:quarto-search.js` (Quarto's UI on top of Pagefind). The
//! keys sort after `js:quarto-nav:*`; within the pair, `autocomplete`
//! sorts before `quarto-search`, so the widget library loads first.
//!
//! The Pagefind runtime is *not* a page script: `quarto-search.js`
//! `import()`s `pagefind.js` from the bundle the index writer emits,
//! so the engine and the index always come from the same build and
//! nothing loads until the reader focuses the box.
//!
//! Every page gets the UI, including pages excluded from the index
//! (`search: false`): the opt-out concerns what is *found*, not where
//! one can search from — Quarto 1 behaves the same way.

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
use crate::project::ProjectKind;
use crate::project::search::config::{ItemContext, SearchOptions};
use crate::project::search::ui::SEARCH_TERMS;
use crate::project::search::{PAGEFIND_BUNDLE_SUBDIR, SEARCH_LIB_SUBDIR, resolve_website_search};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

#[cfg(not(target_arch = "wasm32"))]
const AUTOCOMPLETE_JS: &[u8] = include_bytes!("../../../../resources/search/autocomplete.umd.js");
#[cfg(not(target_arch = "wasm32"))]
const QUARTO_SEARCH_JS: &[u8] = include_bytes!("../../../../resources/search/quarto-search.js");

#[cfg(not(target_arch = "wasm32"))]
const AUTOCOMPLETE_KEY: &str = "js:search:autocomplete.umd.js";
#[cfg(not(target_arch = "wasm32"))]
const AUTOCOMPLETE_REL_PATH: &str = "quarto-search/autocomplete.umd.js";
#[cfg(not(target_arch = "wasm32"))]
const QUARTO_SEARCH_KEY: &str = "js:search:quarto-search.js";
#[cfg(not(target_arch = "wasm32"))]
const QUARTO_SEARCH_REL_PATH: &str = "quarto-search/quarto-search.js";

pub struct SearchUiTransform;

impl SearchUiTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SearchUiTransform {
    fn default() -> Self {
        Self::new()
    }
}

/// The JSON object `quarto-search.js` reads from
/// `<script id="quarto-search-options">`: Quarto 1's keys plus
/// `bundle` (site-root-relative Pagefind bundle path) and `federate`.
pub fn options_json(opts: &SearchOptions, meta: &ConfigValue, bundle: &str) -> serde_json::Value {
    let terms = crate::language::LanguageTerms::from_meta(meta);
    let language: serde_json::Map<String, serde_json::Value> = SEARCH_TERMS
        .iter()
        .map(|(key, default)| {
            let value = terms
                .as_ref()
                .and_then(|t| t.get(key))
                .unwrap_or(default)
                .to_string();
            (key.to_string(), serde_json::Value::String(value))
        })
        .collect();
    let collapse_after = match opts.collapse_after {
        Some(n) => serde_json::json!(n),
        None => serde_json::json!(false),
    };
    let show_item_context = match opts.show_item_context {
        ItemContext::None => serde_json::json!(false),
        ItemContext::Tree => serde_json::json!("tree"),
        ItemContext::Parent => serde_json::json!("parent"),
        ItemContext::Root => serde_json::json!("root"),
    };
    let federate: Vec<serde_json::Value> = opts
        .federate
        .iter()
        .map(|site| {
            let mut m = serde_json::Map::new();
            m.insert("url".into(), serde_json::json!(site.url));
            if let Some(t) = &site.title {
                m.insert("title".into(), serde_json::json!(t));
            }
            if let Some(w) = site.weight {
                m.insert("weight".into(), serde_json::json!(w));
            }
            if let Some(b) = &site.bundle {
                m.insert("bundle".into(), serde_json::json!(b));
            }
            serde_json::Value::Object(m)
        })
        .collect();
    let mut json = serde_json::json!({
        "location": opts.location.as_str(),
        "type": opts.kind.as_str(),
        "panel-placement": opts.panel_placement(),
        "limit": opts.limit,
        "collapse-after": collapse_after,
        "copy-button": opts.copy_button,
        "keyboard-shortcut": opts.keyboard_shortcut,
        "show-item-context": show_item_context,
        "language": language,
        "bundle": bundle,
        "federate": federate,
    });
    if let Some(algolia) = &opts.algolia
        && let Ok(value) = serde_json::to_value(algolia)
    {
        json["algolia"] = value;
    }
    json
}

/// `<meta name="quarto:offset">` + the options script, as one block.
/// `<`, `>`, and `&` inside the JSON are written as `\uXXXX` escapes
/// so no value can terminate the `<script>` element early.
pub fn head_html(options: &serde_json::Value, offset: &str) -> String {
    let json = serde_json::to_string(options)
        .expect("search options serialize")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    format!(
        "<meta name=\"quarto:offset\" content=\"{}\">\n\
         <script id=\"quarto-search-options\" type=\"application/json\">{json}</script>",
        html_escape_attr(offset)
    )
}

fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

#[async_trait::async_trait(?Send)]
impl AstTransform for SearchUiTransform {
    fn name(&self) -> &str {
        "search-ui"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !matches!(
            ctx.project.config.project_kind,
            ProjectKind::Website | ProjectKind::Book
        ) {
            return Ok(());
        }
        if crate::format::lua_format_for(&ctx.format.target_format) != "html" {
            return Ok(());
        }
        // Configuration problems are reported once by the index
        // writer; here only the answer matters.
        let mut scratch = Vec::new();
        let Some(opts) = resolve_website_search(&ast.meta, &mut scratch) else {
            return Ok(());
        };

        let lib_dir = crate::project::orchestrator::project_type_for(ctx.project).lib_dir();
        let bundle = format!("{lib_dir}/{SEARCH_LIB_SUBDIR}/{PAGEFIND_BUNDLE_SUBDIR}/");
        let offset = ctx
            .resource_resolver
            .as_ref()
            .map_or_else(|| "./".to_string(), |r| r.page_url_for_site_root_dir());
        let json = options_json(&opts, &ast.meta, &bundle);
        ast.meta.insert_path(
            &["rendered", "search", "head"],
            ConfigValue::new_string(
                head_html(&json, &offset),
                SourceInfo::generated(By::programmatic_config()),
            ),
        );

        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::artifact::{Artifact, ArtifactScope};
            ctx.artifacts.store(
                AUTOCOMPLETE_KEY,
                Artifact::from_bytes(AUTOCOMPLETE_JS.to_vec(), "text/javascript")
                    .with_path(AUTOCOMPLETE_REL_PATH)
                    .with_scope(ArtifactScope::Project),
            );
            ctx.artifacts.store(
                QUARTO_SEARCH_KEY,
                Artifact::from_bytes(QUARTO_SEARCH_JS.to_vec(), "text/javascript")
                    .with_path(QUARTO_SEARCH_REL_PATH)
                    .with_scope(ArtifactScope::Project),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::search::config::{FederatedSite, SearchLocation, SearchType};

    #[test]
    fn options_json_has_quarto1_keys_plus_bundle_and_federate() {
        let mut opts = SearchOptions::defaults(true);
        opts.federate.push(FederatedSite {
            url: "https://x.example/".into(),
            title: Some("X".into()),
            weight: Some(0.5),
            bundle: None,
        });
        let meta = ConfigValue::null(SourceInfo::for_test());
        let json = options_json(&opts, &meta, "site_libs/quarto-search/pagefind/");
        assert_eq!(json["location"], "navbar");
        assert_eq!(json["type"], "overlay");
        assert_eq!(json["panel-placement"], "end");
        assert_eq!(json["limit"], 50);
        assert_eq!(json["collapse-after"], 3);
        assert_eq!(json["copy-button"], false);
        assert_eq!(
            json["keyboard-shortcut"],
            serde_json::json!(["f", "/", "s"])
        );
        assert_eq!(json["show-item-context"], false);
        assert_eq!(json["language"]["search-label"], "Search");
        assert_eq!(json["language"]["search-text-placeholder"], "");
        assert_eq!(json["bundle"], "site_libs/quarto-search/pagefind/");
        assert_eq!(
            json["federate"],
            serde_json::json!([{"url": "https://x.example/", "title": "X", "weight": 0.5}])
        );
        assert!(json.get("algolia").is_none());
    }

    #[test]
    fn options_json_encodes_collapse_false_and_context_strings() {
        let mut opts = SearchOptions::defaults(false);
        opts.collapse_after = None;
        opts.show_item_context = ItemContext::Parent;
        opts.location = SearchLocation::Sidebar;
        opts.kind = SearchType::Textbox;
        let meta = ConfigValue::null(SourceInfo::for_test());
        let json = options_json(&opts, &meta, "b/");
        assert_eq!(json["collapse-after"], false);
        assert_eq!(json["show-item-context"], "parent");
        assert_eq!(json["panel-placement"], "start");
    }

    #[test]
    fn head_html_escapes_script_terminators() {
        let json = serde_json::json!({"language": {"search-label": "</script><b>&"}});
        let head = head_html(&json, "../");
        assert!(head.starts_with("<meta name=\"quarto:offset\" content=\"../\">\n"));
        assert!(!head.contains("</script><b>"), "{head}");
        assert!(
            head.contains("\\u003c/script\\u003e\\u003cb\\u003e\\u0026"),
            "{head}"
        );
        assert!(head.ends_with("</script>"));
    }
}
