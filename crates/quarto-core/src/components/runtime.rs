/*
 * components/runtime.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The client side of bound components: embedded assets, artifact keys,
 * and the head block (bd-react-components-a8flgldm).
 */

//! Assets and head markup for bound components.
//!
//! Two sets of vendored files ship with a page that binds a component:
//! the React ES modules under `resources/js/react/` (built by
//! `cargo xtask vendor-react`; see that directory's README) and the client
//! runtime `resources/js/quarto-components/quarto-components.js`. Both are
//! embedded at compile time and stored as [`ArtifactScope::Project`]
//! artifacts, so a website writes one copy under `site_libs/` and every
//! page links it; single-document renders resolve Project scope to the
//! page's resource directory.
//!
//! The head block ([`head_block`]) is, in order: an import map (the bare
//! specifiers `react`, `react/jsx-runtime`, `react-dom` and
//! `react-dom/client` resolve to the vendored files, so every module on
//! the page shares one React instance), the manifest the runtime reads
//! (component name to module URL, export and content mode), and the
//! runtime's `<script type="module">`. Order matters: a browser ignores
//! an import map that arrives after the first module script. The block
//! goes in the `header` include slot, which the template emits after the
//! `js:` artifact tags (all classic scripts) and before the body.
//!
//! The artifact keys deliberately avoid the `js:` prefix: that prefix
//! makes `ApplyTemplateStage` emit a classic `<script src>` in the head,
//! and these files are ES modules reached through the import map and
//! the runtime's module tag (the same reasoning as `mermaid:runtime`).

use std::path::Path;

use serde::Serialize;

use crate::artifact::{Artifact, ArtifactScope, ArtifactStore};
use crate::render::RenderContext;

use super::config::ContentMode;

/// The attribute the transform puts on a bound element and the runtime
/// selects on.
pub const COMPONENT_HOST_ATTR: &str = "data-quarto-component";

/// HTML-comment sentinel that keys the head block, so a transform re-run
/// replaces rather than duplicates it (the `append_with_sentinel`
/// contract).
pub const HEAD_SENTINEL: &str = "<!-- quarto-components -->";

/// `id` of the manifest `<script type="application/json">`.
pub const MANIFEST_ID: &str = "quarto-components-manifest";

/// The client runtime, embedded.
const RUNTIME_JS: &str =
    include_str!("../../../../resources/js/quarto-components/quarto-components.js");

/// Versions the vendored React files were built from
/// (`resources/js/react/build-info.json`); pinned by
/// `vendored_react_matches_build_info`.
#[cfg(test)]
const REACT_BUILD_INFO: &str = include_str!("../../../../resources/js/react/build-info.json");

/// One vendored ES module: its bare import specifier, its artifact key
/// and lib-dir path, and its bytes.
struct VendoredModule {
    specifier: &'static str,
    key: &'static str,
    path: &'static str,
    source: &'static str,
}

const REACT_MODULES: &[VendoredModule] = &[
    VendoredModule {
        specifier: "react",
        key: "components:react:react",
        path: "quarto-react/react.js",
        source: include_str!("../../../../resources/js/react/react.js"),
    },
    VendoredModule {
        specifier: "react/jsx-runtime",
        key: "components:react:jsx-runtime",
        path: "quarto-react/react-jsx-runtime.js",
        source: include_str!("../../../../resources/js/react/react-jsx-runtime.js"),
    },
    VendoredModule {
        specifier: "react-dom",
        key: "components:react:react-dom",
        path: "quarto-react/react-dom.js",
        source: include_str!("../../../../resources/js/react/react-dom.js"),
    },
    VendoredModule {
        specifier: "react-dom/client",
        key: "components:react:react-dom-client",
        path: "quarto-react/react-dom-client.js",
        source: include_str!("../../../../resources/js/react/react-dom-client.js"),
    },
];

const RUNTIME: VendoredModule = VendoredModule {
    specifier: "",
    key: "components:runtime",
    path: "quarto-components/quarto-components.js",
    source: RUNTIME_JS,
};

/// Store the runtime and the React modules as project-shared artifacts.
/// Idempotent: the keys are fixed, so a re-run overwrites with identical
/// bytes.
pub fn register_component_assets(artifacts: &mut ArtifactStore) {
    for m in REACT_MODULES.iter().chain(std::iter::once(&RUNTIME)) {
        artifacts.store(
            m.key,
            Artifact::from_string(m.source, "text/javascript")
                .with_path(m.path)
                .with_scope(ArtifactScope::Project),
        );
    }
}

/// Page-relative URL of a lib-dir path. Without a resolver (unit tests,
/// in-memory callers) the bare path, matching the apply-template stage's
/// own fallback.
pub fn asset_url(ctx: &RenderContext<'_>, lib_path: &str) -> String {
    match &ctx.resource_resolver {
        Some(resolver) => resolver.html_url_for(ArtifactScope::Project, Path::new(lib_path)),
        None => lib_path.to_string(),
    }
}

/// One manifest entry: what the runtime needs to mount a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub name: String,
    /// Page-relative URL of the ES module.
    pub module_url: String,
    pub export: Option<String>,
    pub content: ContentMode,
    /// Static props from the declaration, overlaid in the browser by the
    /// element's own.
    pub props: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ManifestComponent<'a> {
    module: &'a str,
    export: &'a str,
    content: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    props: Option<&'a serde_json::Value>,
}

/// The head block for a page: import map, manifest, runtime module tag.
pub fn head_block(ctx: &RenderContext<'_>, entries: &[ManifestEntry]) -> String {
    let imports: serde_json::Map<String, serde_json::Value> = REACT_MODULES
        .iter()
        .map(|m| {
            (
                m.specifier.to_string(),
                serde_json::Value::String(import_map_url(&asset_url(ctx, m.path))),
            )
        })
        .collect();
    let import_map = serde_json::json!({ "imports": imports });

    let components: serde_json::Map<String, serde_json::Value> = entries
        .iter()
        .map(|e| {
            (
                e.name.clone(),
                serde_json::to_value(ManifestComponent {
                    module: &e.module_url,
                    export: e.export.as_deref().unwrap_or("default"),
                    content: e.content.as_str(),
                    props: e.props.as_ref(),
                })
                .expect("manifest entry serializes"),
            )
        })
        .collect();
    let manifest = serde_json::json!({ "components": components });

    format!(
        "{HEAD_SENTINEL}\n\
         <script type=\"importmap\">{}</script>\n\
         <script type=\"application/json\" id=\"{MANIFEST_ID}\">{}</script>\n\
         <script type=\"module\" src=\"{}\"></script>",
        script_json(&import_map),
        script_json(&manifest),
        html_escape_attr(&asset_url(ctx, RUNTIME.path)),
    )
}

/// An import map value must be an absolute URL or start with `/`, `./`
/// or `../`; a bare relative path such as `site_libs/x.js` is read as a
/// bare specifier and the browser ignores the whole entry. The resolver
/// hands back the bare form for a root page, so prefix it.
fn import_map_url(url: &str) -> String {
    if url.starts_with('/')
        || url.starts_with("./")
        || url.starts_with("../")
        || url.contains("://")
    {
        url.to_string()
    } else {
        format!("./{url}")
    }
}

/// JSON for an inline `<script>`: `<`, `>` and `&` are written as
/// `\uXXXX` escapes so no value can close the script element or start a
/// comment, whatever the author put in a name (the same rule as the
/// listing bootstrap's `script_json`).
pub fn script_json(value: &serde_json::Value) -> String {
    value
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use crate::resource_resolver::ResourceResolverContext;

    fn make_test_project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        }
    }

    fn entry(name: &str, url: &str) -> ManifestEntry {
        ManifestEntry {
            name: name.into(),
            module_url: url.into(),
            export: None,
            content: ContentMode::Dom,
            props: None,
        }
    }

    #[test]
    fn manifest_carries_static_props_when_declared() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut e = entry("c", "quarto-components/c.js");
        e.props = Some(serde_json::json!({"interval": 3000}));
        let block = head_block(&ctx, &[e]);
        assert!(
            block.contains("\"c\":{\"content\":\"dom\",\"export\":\"default\",\"module\":\"quarto-components/c.js\",\"props\":{\"interval\":3000}}"),
            "{block}"
        );
    }

    /// The committed React files carry the versions `build-info.json`
    /// records, so a `react`/`react-dom` bump that skips
    /// `cargo xtask vendor-react` fails here.
    #[test]
    fn vendored_react_matches_build_info() {
        let info: serde_json::Value = serde_json::from_str(REACT_BUILD_INFO).unwrap();
        let react = info["react"].as_str().expect("react version");
        let react_dom = info["react-dom"].as_str().expect("react-dom version");
        for m in REACT_MODULES {
            let version = if m.specifier.starts_with("react-dom") {
                react_dom
            } else {
                react
            };
            let banner = format!("/* {}@{version} ", m.specifier);
            assert!(
                m.source.starts_with(&banner),
                "{} must start with the banner {banner:?}; starts with {:?}",
                m.path,
                &m.source[..m.source.len().min(60)]
            );
        }
        // One React instance per page: the subpath modules import the
        // bare specifier instead of bundling their own copy.
        for m in &REACT_MODULES[2..] {
            assert!(
                m.source.contains("from\"react\""),
                "{} must import react through the import map",
                m.path
            );
        }
        // No leftover CommonJS `require` shim: that is what a browser
        // cannot satisfy (the resolver shim in build.mjs exists for it).
        for m in REACT_MODULES {
            assert!(
                !m.source.contains("Dynamic require"),
                "{} must not carry esbuild's dynamic-require shim",
                m.path
            );
        }
    }

    #[test]
    fn runtime_imports_through_the_import_map_only() {
        assert!(RUNTIME_JS.contains("from \"react\""));
        assert!(RUNTIME_JS.contains("from \"react-dom/client\""));
        assert!(RUNTIME_JS.contains(COMPONENT_HOST_ATTR));
        assert!(RUNTIME_JS.contains(MANIFEST_ID));
    }

    #[test]
    fn assets_are_project_scoped_and_not_template_scripts() {
        let mut store = ArtifactStore::new();
        register_component_assets(&mut store);
        for m in REACT_MODULES.iter().chain(std::iter::once(&RUNTIME)) {
            let a = store.get(m.key).expect(m.key);
            assert_eq!(a.scope, ArtifactScope::Project);
            assert_eq!(a.path.as_deref(), Some(Path::new(m.path)));
            assert_eq!(a.content_type, "text/javascript");
            assert!(
                !m.key.starts_with("js:"),
                "{} would get a classic <script> tag from the template",
                m.key
            );
        }
        // Idempotent.
        let before = store.len();
        register_component_assets(&mut store);
        assert_eq!(store.len(), before);
    }

    #[test]
    fn head_block_orders_import_map_manifest_runtime_with_page_relative_urls() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let resolver =
            ResourceResolverContext::website("/site", "/site/docs/page.html", "site_libs", "page");
        let ctx =
            RenderContext::new(&project, &doc, &format, &binaries).with_resource_resolver(resolver);
        let block = head_block(
            &ctx,
            &[entry(
                "counter",
                "../site_libs/quarto-components/counter.js",
            )],
        );
        let import_map = block.find("<script type=\"importmap\">").unwrap();
        let manifest = block.find("id=\"quarto-components-manifest\"").unwrap();
        let runtime = block
            .find("<script type=\"module\" src=\"../site_libs/quarto-components/quarto-components.js\">")
            .unwrap();
        assert!(block.starts_with(HEAD_SENTINEL));
        assert!(import_map < manifest && manifest < runtime, "{block}");
        assert!(
            block.contains("\"react\":\"../site_libs/quarto-react/react.js\""),
            "{block}"
        );
        assert!(
            block
                .contains("\"react-dom/client\":\"../site_libs/quarto-react/react-dom-client.js\""),
            "{block}"
        );
        assert!(
            block.contains(
                "\"counter\":{\"content\":\"dom\",\"export\":\"default\",\"module\":\"../site_libs/quarto-components/counter.js\"}"
            ),
            "{block}"
        );
    }

    #[test]
    fn head_block_without_resolver_uses_bare_paths() {
        let project = make_test_project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let block = head_block(&ctx, &[]);
        assert!(
            block.contains("\"react\":\"./quarto-react/react.js\""),
            "{block}"
        );
        assert!(block.contains("src=\"quarto-components/quarto-components.js\""));
        assert!(block.contains("{\"components\":{}}"));
    }

    #[test]
    fn import_map_values_are_never_bare_specifiers() {
        assert_eq!(import_map_url("site_libs/x.js"), "./site_libs/x.js");
        assert_eq!(import_map_url("../site_libs/x.js"), "../site_libs/x.js");
        assert_eq!(import_map_url("./x.js"), "./x.js");
        assert_eq!(import_map_url("/x.js"), "/x.js");
        assert_eq!(import_map_url("https://h/x.js"), "https://h/x.js");
    }

    #[test]
    fn script_json_escapes_markup_characters() {
        let v = serde_json::json!({"a": "</script><!-- x --> & y"});
        let s = script_json(&v);
        assert!(
            !s.contains('<') && !s.contains('>') && !s.contains('&'),
            "{s}"
        );
        let back: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }
}
