/*
 * project/search/index_writer.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pagefind bundle construction for website search (bd-eip0naf4).
 */

//! Build a Pagefind bundle in memory from already-rendered pages.
//!
//! [`build_bundle`] is the single point of contact with the
//! `pagefind` crate. It takes the pages as HTML strings (the capture
//! half of `project::search` produces them), runs Pagefind's indexer,
//! and returns the bundle as in-memory files whose paths are relative
//! to the bundle directory. Writing them through `SystemRuntime` is
//! the caller's job, so this module never touches the filesystem.
//!
//! ## Executor bridging
//!
//! Pagefind's library API (`pagefind::api::PagefindIndex`) is async
//! and parks its CPU work on `tokio::task::spawn_blocking`, which
//! requires a tokio runtime on the calling thread. The render
//! pipeline is driven by two different executors: `q2 render` uses
//! `pollster::block_on` (no tokio context at all) and `q2 preview`
//! runs inside a tokio multi-thread runtime (where building a nested
//! runtime panics). [`build_bundle`] therefore runs the whole
//! indexing job on a dedicated OS thread with its own current-thread
//! tokio runtime and joins it. That works identically under either
//! caller, keeps Pagefind's rayon fan-out off the pipeline thread,
//! and costs one thread spawn per site render.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use pagefind::api::PagefindIndex;

/// One page to index.
#[derive(Debug, Clone)]
pub struct IndexPage {
    /// Site-root-relative URL the result links to (`guide/intro.html`,
    /// no leading slash). Stored verbatim: Pagefind's `index.html`
    /// stripping is disabled so the bundle and `search.json` agree.
    pub url: String,
    /// A complete HTML document (`<html lang>` … `</html>`). The
    /// capture half synthesizes the wrapper around the rendered
    /// body; Pagefind reads the language from the `lang` attribute
    /// and the title from `data-pagefind-meta`.
    pub html: String,
}

/// One file of the built bundle; `path` is relative to the bundle
/// directory (`pagefind-entry.json`, `index/<hash>.pf_index`, …).
#[derive(Debug, Clone)]
pub struct BundleFile {
    pub path: PathBuf,
    pub contents: Vec<u8>,
}

/// Summary returned with the bundle.
#[derive(Debug, Clone, Default)]
pub struct BundleStats {
    /// Pages fed to Pagefind.
    pub pages: usize,
    /// Total word count Pagefind reported across those pages.
    pub words: u64,
}

/// A built bundle.
#[derive(Debug, Clone, Default)]
pub struct Bundle {
    pub files: Vec<BundleFile>,
    pub stats: BundleStats,
    /// The Pagefind version recorded in the bundle's
    /// `pagefind-entry.json` (`"1.5.2"`), i.e. the index format and
    /// the embedded runtime. Recorded in `manifest.json` so a
    /// federated parent can refuse to merge a child built with a
    /// different format. Empty for an empty bundle.
    pub pagefind_version: String,
}

/// Build the Pagefind bundle for `pages` in memory.
///
/// Returns an empty [`Bundle`] (no files) when `pages` is empty:
/// Pagefind itself errors on an empty site, and a website with
/// nothing to index should simply get no bundle.
pub fn build_bundle(pages: Vec<IndexPage>) -> Result<Bundle, String> {
    if pages.is_empty() {
        return Ok(Bundle::default());
    }
    // See the module docs for why this runs on its own thread.
    let handle = std::thread::Builder::new()
        .name("quarto-search-index".into())
        .spawn(move || -> Result<Bundle, String> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("could not start the search indexer runtime: {e}"))?;
            runtime.block_on(build_bundle_async(pages))
        })
        .map_err(|e| format!("could not spawn the search indexer thread: {e}"))?;
    handle
        .join()
        .map_err(|_| "the search indexer thread panicked".to_string())?
}

/// Pagefind ships its own UIs and highlighter with every bundle
/// (`pagefind-ui.*`, `pagefind-modular-ui.*`, `pagefind-component-ui.*`,
/// `pagefind-highlight.js`; ~300 KB). Quarto has its own search UI and
/// its own target-page highlighter, so none of these are served.
/// Kept: `pagefind.js`, `pagefind-worker.js`, the wasm runtimes, the
/// entry/meta files, and the index/fragment/filter chunks.
fn is_pagefind_ui_asset(path: &std::path::Path) -> bool {
    let name = path.to_string_lossy();
    name.starts_with("pagefind-ui.")
        || name.starts_with("pagefind-modular-ui.")
        || name.starts_with("pagefind-component-ui.")
        || name == "pagefind-highlight.js"
}

async fn build_bundle_async(pages: Vec<IndexPage>) -> Result<Bundle, String> {
    let config = pagefind::options::PagefindServiceConfig::builder()
        // Quarto 1 hrefs keep `index.html`; `search.json` and the
        // bundle must link to the same place.
        .keep_index_url(true)
        .build();
    let mut index = PagefindIndex::new(Some(config))
        .map_err(|e| format!("could not configure the search indexer: {e}"))?;

    let mut stats = BundleStats::default();
    for page in pages {
        let added = index
            .add_html_file(None, Some(page.url.clone()), page.html)
            .await
            .map_err(|e| format!("could not index `{}` for search: {e}", page.url))?;
        stats.pages += 1;
        stats.words += u64::from(added.page_word_count);
    }
    // Pagefind refuses to build an index with no pages at all; with at
    // least one page it always builds (an all-empty page still counts
    // one word), so from here on any error is a real one.

    let files = index
        .get_files()
        .await
        .map_err(|e| format!("could not build the search index: {e}"))?;
    let files: Vec<BundleFile> = files
        .into_iter()
        .filter(|f| !is_pagefind_ui_asset(&f.filename))
        .map(|f| BundleFile {
            path: f.filename,
            contents: f.contents,
        })
        .collect();
    let pagefind_version = files
        .iter()
        .find(|f| f.path.to_string_lossy() == "pagefind-entry.json")
        .and_then(|f| serde_json::from_slice::<serde_json::Value>(&f.contents).ok())
        .and_then(|v| v["version"].as_str().map(str::to_owned))
        .ok_or_else(|| "the search index bundle has no pagefind-entry.json".to_string())?;
    Ok(Bundle {
        files,
        stats,
        pagefind_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(url: &str, title: &str, body: &str) -> IndexPage {
        IndexPage {
            url: url.to_string(),
            html: format!(
                "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
                 <title>{title}</title></head>\
                 <body><h1>{title}</h1>{body}</body></html>"
            ),
        }
    }

    fn has_file(bundle: &Bundle, pred: impl Fn(&str) -> bool) -> bool {
        bundle
            .files
            .iter()
            .any(|f| pred(&f.path.to_string_lossy().replace('\\', "/")))
    }

    #[test]
    fn empty_input_yields_no_bundle() {
        let bundle = build_bundle(vec![]).unwrap();
        assert!(bundle.files.is_empty());
        assert_eq!(bundle.stats.pages, 0);
    }

    #[test]
    fn single_page_bundle_has_runtime_index_and_fragment_files() {
        let bundle = build_bundle(vec![page(
            "guide/intro.html",
            "Introduction",
            "<h2 id=\"setup\">Setup</h2><p>Install the quarto command line tool.</p>",
        )])
        .unwrap();
        assert_eq!(bundle.stats.pages, 1);
        assert!(bundle.stats.words > 0);
        assert!(has_file(&bundle, |p| p == "pagefind-entry.json"));
        assert!(has_file(&bundle, |p| p == "pagefind.js"));
        assert!(has_file(&bundle, |p| p == "pagefind-worker.js"));
        assert!(!has_file(&bundle, |p| p.starts_with("pagefind-ui.")));
        assert!(!has_file(&bundle, |p| p == "pagefind-highlight.js"));
        assert!(has_file(&bundle, |p| p == "wasm.en.pagefind"));
        assert!(has_file(&bundle, |p| p.ends_with(".pf_meta")));
        assert!(has_file(&bundle, |p| p.starts_with("index/")
            && p.ends_with(".pf_index")));
        assert!(has_file(&bundle, |p| p.starts_with("fragment/")
            && p.ends_with(".pf_fragment")));

        // The entry file records the crate version we pin, so the
        // manifest and the runtime cannot drift apart.
        let entry = bundle
            .files
            .iter()
            .find(|f| f.path.to_string_lossy() == "pagefind-entry.json")
            .unwrap();
        let entry: serde_json::Value = serde_json::from_slice(&entry.contents).unwrap();
        assert_eq!(
            entry["version"].as_str(),
            Some(bundle.pagefind_version.as_str())
        );
        assert_eq!(bundle.pagefind_version, "1.5.2");
        assert!(entry["languages"]["en"].is_object());
    }

    #[test]
    fn explicit_url_is_kept_verbatim_including_index_html() {
        let bundle = build_bundle(vec![page(
            "docs/index.html",
            "Docs",
            "<p>Documentation home page with enough words to index.</p>",
        )])
        .unwrap();
        // Fragments are gzip-compressed CBOR; the URL survives as a
        // plain byte string inside, so decompress and look for it.
        let fragment = bundle
            .files
            .iter()
            .find(|f| f.path.to_string_lossy().starts_with("fragment/"))
            .unwrap();
        let mut decoder = flate2::read::GzDecoder::new(fragment.contents.as_slice());
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut bytes).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("docs/index.html"),
            "fragment should carry the verbatim url; got: {text}"
        );
    }

    /// Pagefind never drops a page it was handed, even one with no
    /// text (it reports one word for it). The capture half always
    /// supplies a title, so this is documentation of the engine's
    /// behaviour rather than a path we rely on.
    #[test]
    fn page_without_text_still_indexes() {
        let bundle = build_bundle(vec![IndexPage {
            url: "empty.html".into(),
            html: "<!DOCTYPE html><html lang=\"en\"><head></head><body></body></html>".into(),
        }])
        .unwrap();
        assert_eq!(bundle.stats.pages, 1);
        assert!(!bundle.files.is_empty());
    }

    /// `q2 preview` drives the pipeline from inside a tokio
    /// multi-thread runtime; building a nested runtime on that thread
    /// would panic, so the dedicated-thread bridge must hold there too.
    #[test]
    fn builds_from_inside_a_tokio_runtime() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let bundle = rt
            .block_on(async {
                build_bundle(vec![page(
                    "a.html",
                    "A",
                    "<p>Some words for the index to chew on.</p>",
                )])
            })
            .unwrap();
        assert_eq!(bundle.stats.pages, 1);
    }
}
