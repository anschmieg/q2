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
use crate::project::search::config::{AlgoliaOptions, ItemContext, SearchOptions};
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
#[cfg(not(target_arch = "wasm32"))]
const ALGOLIA_PRESET_JS: &[u8] =
    include_bytes!("../../../../resources/search/autocomplete-preset-algolia.umd.js");
#[cfg(not(target_arch = "wasm32"))]
const ALGOLIA_PRESET_KEY: &str = "js:search:z-autocomplete-preset-algolia.umd.js";
#[cfg(not(target_arch = "wasm32"))]
const ALGOLIA_PRESET_REL_PATH: &str = "quarto-search/autocomplete-preset-algolia.umd.js";
#[cfg(not(target_arch = "wasm32"))]
const ALGOLIA_LOGO_SVG: &[u8] =
    include_bytes!("../../../../resources/search/search-by-algolia.svg");
/// Not a `js:`/`css:` key: flushed as a file, never linked from the head.
#[cfg(not(target_arch = "wasm32"))]
const ALGOLIA_LOGO_KEY: &str = "search:search-by-algolia.svg";
#[cfg(not(target_arch = "wasm32"))]
const ALGOLIA_LOGO_REL_PATH: &str = "quarto-search/search-by-algolia.svg";

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
    if let Some(algolia) = &opts.algolia {
        json["algolia"] = algolia_json(algolia, lib_dir_of(bundle));
    }
    json
}

/// The `algolia` object of the options JSON (Quarto 1's
/// `algoliaOptions` plus the `libDir` the logo is served from and the
/// cookie-consent flag, which is always `false` here: q2 has no
/// cookie-consent feature yet, so the insights scripts load unguarded).
fn algolia_json(algolia: &AlgoliaOptions, lib_dir: &str) -> serde_json::Value {
    serde_json::json!({
        "application-id": algolia.application_id,
        "search-only-api-key": algolia.search_only_api_key,
        "index-name": algolia.index_name,
        "params": algolia.params.as_ref().map(config_value_to_json),
        "index-fields": algolia.index_fields.as_ref().map(config_value_to_json),
        "analytics-events": algolia.analytics_events,
        "show-logo": algolia.show_logo,
        "libDir": lib_dir,
        "cookie-consent-enabled": false,
    })
}

/// The lib dir is the first segment of the bundle path
/// (`site_libs/quarto-search/pagefind/` → `site_libs`).
fn lib_dir_of(bundle: &str) -> &str {
    bundle.split('/').next().unwrap_or(bundle)
}

/// Plain JSON for a configuration subtree (scalars, arrays, maps;
/// parsed inline text flattens to a string).
fn config_value_to_json(cv: &ConfigValue) -> serde_json::Value {
    use quarto_pandoc_types::ConfigValueKind;
    use yaml_rust2::Yaml;
    match &cv.value {
        ConfigValueKind::Scalar { yaml, .. } => match yaml {
            Yaml::Boolean(b) => serde_json::json!(b),
            Yaml::Integer(i) => serde_json::json!(i),
            Yaml::Real(r) => r
                .parse::<f64>()
                .map_or_else(|_| serde_json::json!(r), |f| serde_json::json!(f)),
            Yaml::Null => serde_json::Value::Null,
            other => serde_json::json!(other.as_str().map(str::to_owned).unwrap_or_default()),
        },
        ConfigValueKind::Array(items) => {
            serde_json::Value::Array(items.iter().map(config_value_to_json).collect())
        }
        ConfigValueKind::Map(entries) => serde_json::Value::Object(
            entries
                .iter()
                .map(|e| (e.key.clone(), config_value_to_json(&e.value)))
                .collect(),
        ),
        _ => serde_json::json!(cv.as_plain_text().unwrap_or_default()),
    }
}

/// Head markup for the Algolia backend (Quarto 1's
/// `websiteSearchIncludeInHeader`): the search client from the CDN and,
/// with `analytics-events`, the insights loader and the Autocomplete
/// insights plugin. Versions follow Quarto 1's `configuration` pins
/// where it had one (`algoliasearch@4.5.1`); the two insights scripts
/// are unpinned there too.
fn algolia_head_html(algolia: &AlgoliaOptions) -> String {
    let mut head = String::from(
        "<script src=\"https://cdn.jsdelivr.net/npm/algoliasearch@4.5.1/dist/algoliasearch-lite.umd.js\"></script>\n",
    );
    if algolia.analytics_events {
        head.push_str(
            "<script>var ALGOLIA_INSIGHTS_SRC = \"https://cdn.jsdelivr.net/npm/search-insights/dist/search-insights.iife.min.js\";\n\
             !function(e,a,t,n,s,i,c){e.AlgoliaAnalyticsObject=s,e[s]=e[s]||function(){\n\
             (e[s].queue=e[s].queue||[]).push(arguments)},i=a.createElement(t),c=a.getElementsByTagName(t)[0],\n\
             i.async=1,i.src=n,c.parentNode.insertBefore(i,c)\n\
             }(window,document,\"script\",ALGOLIA_INSIGHTS_SRC,\"aa\");</script>\n\
             <script src=\"https://cdn.jsdelivr.net/npm/@algolia/autocomplete-plugin-algolia-insights\"></script>\n",
        );
    }
    head
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
        let mut head = head_html(&json, &offset);
        if let Some(algolia) = &opts.algolia {
            head.push('\n');
            head.push_str(algolia_head_html(algolia).trim_end());
        }
        ast.meta.insert_path(
            &["rendered", "search", "head"],
            ConfigValue::new_string(head, SourceInfo::generated(By::programmatic_config())),
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
            if let Some(algolia) = &opts.algolia {
                // Sorts after `quarto-search.js`, which is fine: the
                // preset is only called from inside a search.
                ctx.artifacts.store(
                    ALGOLIA_PRESET_KEY,
                    Artifact::from_bytes(ALGOLIA_PRESET_JS.to_vec(), "text/javascript")
                        .with_path(ALGOLIA_PRESET_REL_PATH)
                        .with_scope(ArtifactScope::Project),
                );
                if algolia.show_logo {
                    ctx.artifacts.store(
                        ALGOLIA_LOGO_KEY,
                        Artifact::from_bytes(ALGOLIA_LOGO_SVG.to_vec(), "image/svg+xml")
                            .with_path(ALGOLIA_LOGO_REL_PATH)
                            .with_scope(ArtifactScope::Project),
                    );
                }
            }
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
    fn algolia_options_serialize_with_lib_dir_and_consent_flag() {
        let mut opts = SearchOptions::defaults(true);
        opts.algolia = Some(AlgoliaOptions {
            application_id: "app".into(),
            search_only_api_key: "key".into(),
            index_name: "idx".into(),
            params: None,
            index_fields: None,
            analytics_events: true,
            show_logo: true,
        });
        let meta = ConfigValue::null(SourceInfo::for_test());
        let json = options_json(&opts, &meta, "site_libs/quarto-search/pagefind/");
        assert_eq!(json["algolia"]["application-id"], "app");
        assert_eq!(json["algolia"]["index-name"], "idx");
        assert_eq!(json["algolia"]["libDir"], "site_libs");
        assert_eq!(json["algolia"]["cookie-consent-enabled"], false);
        assert_eq!(json["algolia"]["analytics-events"], true);
        assert_eq!(json["algolia"]["params"], serde_json::Value::Null);
        let head = algolia_head_html(opts.algolia.as_ref().unwrap());
        assert!(head.contains("algoliasearch@4.5.1/dist/algoliasearch-lite.umd.js"));
        assert!(head.contains("search-insights.iife.min.js"));
        assert!(head.contains("autocomplete-plugin-algolia-insights"));
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
