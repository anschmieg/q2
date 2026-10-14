---
title: 'Client-side full-text search for Quarto 2 websites: Quarto 1 port + engine survey (bd-eip0naf4)'
date: 2026-10-14
description: 'Assessment for porting Quarto 1 website search to Quarto 2 with a modern engine in place of Fuse.js + monolithic search.json: what Quarto 1 does, what q2 already has, measured index sizes, a survey of client-side search engines in 2026, and a recommended architecture (Pagefind as a native Rust library) with open design questions.'
status: draft  # Exploration — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-eip0naf4
---

**Strand:** bd-eip0naf4 (epic; child of the websites epic bd-0tr6; related: bd-mdet0yoy hub-client FTS, bd-ulgr JS dependency design, bd-6cme sidebar search).
**Branch:** `braid/bd-eip0naf4-website-search` (topic branch in the main checkout, based on `main` @ `80faa63f4`; no worktree).
**Do not start implementation until the user gives the go-ahead.**

Investigation artifacts (all under `claude-notes/plans/website-search-investigation/`):

- `quarto1-search-implementation.md` — full read of Quarto 1's `website-search.ts` / `quarto-search.js` / schema / templates, with file:line refs.
- `q2-infrastructure-map.md` — where search plugs into q2's pipeline, navigation, asset injection, config, i18n, preview, and the WASM constraint.
- `engine-survey-2026-10.md` — thirteen candidate engines against five criteria, comparison table, what peer SSGs use in 2026.
- `quarto1-search-json-sizes.md` — measured `search.json` sizes on live Quarto 1 sites.
- `pagefind-spike/` — scratch crate that indexes quarto.org's records with the `pagefind` crate as a library; build findings and measurements.

## Triage verdict

**Ready to design.** The user-facing surface is a straight port of Quarto 1 (its YAML keys, placement, overlay UI, and per-section results are well understood and documented in the artifacts). The implementation should *not* be a port: the recommended engine is **Pagefind**, consumed as a native Rust library crate at `q2 render` time, with Quarto's own UI on top of its JS API. The open questions below are about where text is captured, what the browser preview gets, and how much of Quarto 1's UI stack (Algolia Autocomplete) we keep.

## Issue context

Quarto 1 ships client-side website search built on Fuse.js. At render time it writes one `search.json` (one record per page plus one per level-2 section) and, on the first keystroke, the browser downloads the whole file and builds a Fuse index in memory. Fuse is a linear Bitap scanner (no inverted index, no ranking model, no stemming), so for large sites both the download and each query are slow; the code even contains a 125 ms "sub-index" workaround. Quarto 2 has no website search yet. The user wants the port to keep Quarto 1's syntax and functionality while switching to a modern engine, and asked for a survey of the state of the art.

## Dependency graph

- **parent-child: bd-0tr6** (websites epic) — search is the last large website feature missing from q2.
- **related: bd-mdet0yoy** (hub-client full-text search) — a *different* feature: in-editor search over project *sources* in hub-client, shipped on MiniSearch 7. Its deferred Phase B (bd-b5ub29xd) wants per-document plaintext extraction and DocumentProfile boosting, which overlaps with what a site index needs; the extractor could be shared.
- **related: bd-ulgr** (JS dependency handling design, open P1) — its text explicitly lists search. The de facto answer today is the `js:*` project-artifact pattern used by Bootstrap/clipboard/tabsets/listjs; this plan follows it.
- **related: bd-6cme** (sidebar search integration, open) — "depends on the separate search epic"; this is that epic. Sidebar `search:` is not parsed yet.
- `book/config.rs:72` already lists `search` among the website keys books share, so books come along for free at the config layer.

## What Quarto 1 does (summary; details in `quarto1-search-implementation.md`)

**Config surface** (`website.search`, `anyOf: [boolean, object]`, closed object):

| key | default (code) | notes |
|---|---|---|
| `location` | `navbar` if there is a navbar, else `sidebar` | |
| `type` | `overlay` for navbar, `textbox` for sidebar | textbox becomes the overlay modal below `lg` |
| `limit` | **50** (docs say 20) | caps *sections*, not documents |
| `collapse-after` | **3** (docs say 2) | rows per document before "N more matches"; `false` disables |
| `copy-button` | `false` | clipboard button copying `?q=…&show-results=1` |
| `keyboard-shortcut` | `["f","/","s"]` | |
| `show-item-context` | `false` | `tree` / `parent` / `root` / bool; shows sidebar crumbs |
| `merge-navbar-crumbs` | `true` | index-time only |
| `algolia` | — | `index-name`, `application-id`, `search-only-api-key`, `index-fields`, `analytics-events`, `show-logo`, `params` |

Plus `navbar.search` / `sidebar.search` booleans, per-page `search: false`, drafts excluded, the `.quarto-include-in-search-index` class, and eleven `search-*` language terms.

**Index.** Post-render, Quarto 1 re-reads each HTML output with deno-dom, strips `#title-block-header`, the TOC nav, `script`/`style`, then emits one record for the page body and one per `section.level2` (h3+ folded into their h2; content outside level-2 sections partly lost). Records: `{objectID, href, title, section, text, crumbs}`; `text` is uncapped `textContent` including code. Incremental renders upsert by href and never prune stale anchors.

**Runtime.** Algolia Autocomplete 1.19.1 (vendored, ~120 KB) drives the UI; Fuse 6.6.2 with keys title/section weighted 20, text 10, `threshold: 0.1`, `ignoreLocation: true`. Results are grouped by document, collapsed after N, a literal-substring snippet is highlighted, and the target page gets `?q=` which a custom highlighter (not mark.js) uses to `<mark>` matches, opening tabsets that contain them. Dark mode comes from `quarto-search.scss` being a Sass layer on the Bootstrap bundle.

## What q2 has today (summary; details in `q2-infrastructure-map.md`)

- **Pipeline.** Two passes: Pass 1 builds a `DocumentProfile` per file (title, outline with heading ids, description, keywords, categories; *no body text*, and pre-engine), Pass 2 renders. `ProjectType::post_render` / `post_resources` run after Pass 2 with the project `ArtifactStore`.
- **The model to copy is llms.txt**: `LlmsCaptureTransform` at the tail of the Finalization phase stores a path-less, project-scoped artifact per page; `write_llms_artifacts` in `post_resources` iterates `index.profiles()` and reads them back; a manifest under `.quarto/` lets `RenderMode::Subset` reuse captures for pages skipped this run.
- **Do not read rendered HTML back from disk.** `post_render_upgrade` (listings) forbids reuse for search in its module header, and its `scraper` dependency is native-only.
- **Navigation.** `Navbar.search: bool` is already parsed and `navbar_to_html` already emits `<div class="quarto-search"></div>`; the secondary nav deliberately omits the search button because `window.quartoOpenSearch()` does not exist. `Sidebar` has no `search` field (bd-6cme).
- **Assets.** Vendored JS lives under `resources/js/<lib>/` with `include_bytes!` and a `js:<key>` project artifact (`ApplyTemplateStage` emits script tags in sorted key order); listjs is the closest analogue. SCSS components are quarto-sass layers (`load_listing_layer()` → add `load_search_layer()`).
- **Config.** No schema layer; typed resolvers (`resolve_website_value`, `resolve_website_bool`). Diagnostics codes go in the error catalog.
- **i18n.** All eleven `search-*` terms already exist in `resources/language/_language.yml` and 35 translations.
- **WASM / hub preview.** `wasm-quarto-hub-client` runs the project pipeline in `RenderMode::ActivePage`: Pass 1 for every file, Pass 2 for the active page only, and the native-only `post_render` hooks (sitemap, feeds, llms) do not run. **The browser preview never renders a whole site today**, so a site-wide full-text index cannot be built there regardless of engine. Anything added to `quarto-core` must still compile on wasm32 or be cfg-gated the way `scraper` is.
- **Preview.** `q2 preview --static` serves `_site` as plain files with `Cache-Control: no-store`; index files would just be served. Default `q2 preview` is the WASM path above.

## Measurements

Quarto 1 `search.json` on live sites (`quarto1-search-json-sizes.md`):

| site | pages | records | raw | gzip |
|---|---:|---:|---:|---:|
| quarto.org | 241 | 1738 | 2.54 MB | 591 KB |
| r4ds.hadley.nz | 38 | 254 | 977 KB | 275 KB |
| mlr3book | 23 | 167 | 1.08 MB | 306 KB |

The whole file is fetched on the first keystroke. A 5,000-page site at quarto.org's density extrapolates to ~50 MB raw / ~12 MB gz.

Pagefind as a library on the same quarto.org records (`pagefind-spike/README.md`): indexing 241 pages takes ~225 ms; output is 26 index chunks of ~33 KB each (already gzipped) + 241 per-page fragments (median 2.2 KB) + 2.4 KB meta + 68 KB wasm + ~13 KB gz JS. A first query transfers ~120–160 KB; each later query touching a new alphabetical range adds ~33 KB. On a 5,000-page site the per-query cost stays in that band.

## Engine survey (summary; details in `engine-survey-2026-10.md`)

Criteria: C1 index buildable from Rust (wasm32-clean or gatable); C2 chunked / lazy index at 1k–5k pages; C3 ranking, prefix, fuzzy, stemming, CJK, snippets; C4 runtime size, license, maintenance; C5 API for our own UI.

| engine | C1 Rust indexer | C2 chunked | C3 | verdict |
|---|---|---|---|---|
| **Pagefind 1.5.2** (MIT) | `pagefind` crate works natively as a library; **fails on wasm32** (tokio fs/rt-multi-thread, rayon); format is undocumented CBOR+gzip but reimplementable (Pagefind.Net precedent) | **yes**: alphabetical word-range chunks + per-page fragments | BM25-aligned, prefix, Snowball stemming ×33 langs, CJK (extended build + `Intl.Segmenter`), excerpts, per-heading `sub_results`; **no typo tolerance** | **recommended** |
| MiniSearch 7.2 (MIT) | good: serialized format is explicitly meant for other-language writers | no (corpus-sized JSON) | BM25+, prefix, fuzzy, boosts, custom tokenizer | fallback / small-site option; already in hub-client |
| FlexSearch 0.8 | poor (undocumented export, all keys required) | no | fast, idiosyncratic API | no |
| Lunr / Elasticlunr | `elasticlunr-rs` exists (mdBook, Zola) | no; large | TF-IDF, stale JS | no |
| Orama 3 | persistence plugin, not a contract | no | BM25, typo tolerance | no (vendor pivot to hosted) |
| tinysearch | index compiled *into* wasm; needs cargo at site build | no | no ranking | no |
| Stork | dead (2023) | | | no |
| tantivy / Summa | tantivy maintainers: browser "out of scope" | HTTP range | full | no |
| SQLite FTS5 + range VFS | SQLite-in-renderer, hosting-dependent | HTTP range | FTS5 | no |
| docfind (Microsoft, 2026) | plausible, unverified; not on crates.io | no (single multi-MB wasm) | RAKE, prefix, typos | watch |
| Fuse.js 7.5 | trivial | no | linear scan | baseline only |

Peers in 2026: Astro Starlight and Nextra 4 default to Pagefind; VitePress and Jupyter Book 2 use MiniSearch; mdBook and Zola use elasticlunr-rs; MkDocs Material uses lunr in a worker; Docusaurus is Algolia-first; rustdoc and Sphinx roll their own compact formats.

## Recommendation

**Engine: Pagefind, consumed as a native Rust library.** It is the only candidate that keeps per-query transfer roughly constant as the site grows, it ships stemming and CJK handling we would otherwise have to build, its per-heading `sub_results` map directly onto Quarto 1's document-with-sections result groups, and it is where the static-site ecosystem has converged. Using the crate (not the CLI) means no subprocess, no Node, and `get_files()` returns the output in memory so it can be written through `SystemRuntime`. Pinning one crate version pins the matching `pagefind_web` runtime it embeds, so there is no format drift to manage.

**What we give up, stated plainly:**
- *Typo tolerance.* Pagefind has prefix matching and stemming but no fuzzy matching. Quarto 1's Fuse `threshold: 0.1` was close to exact anyway, so the practical regression is small, but it is a regression.
- *wasm32.* The indexer cannot run in the browser build. Today that costs nothing (the browser preview never renders a whole site), but it closes the door on a future "publish a full site from the hub" path unless we fork the ~3.4k-line indexer core or write our own `pf_*` writer. The plan cfg-gates the dependency like `scraper` and records the fork/reimplement routes as a known follow-up rather than paying for them now.
- *Hosting constraints.* The runtime needs `'wasm-unsafe-eval'` in CSP and does not work from `file://` (Quarto 1 did not either; it alerted).
- *Dependency weight.* The crate adds `lol_html`, `minicbor`, `rayon`, and ~700 KB of embedded UI assets to the native binary; `default-features = false` is required (drops actix) and the consumer must enable tokio's `io-util` (build finding in the spike, worth an upstream PR). Exact binary/compile-time cost in the real tree is a Phase 0 measurement.

**UI: Quarto's own, ported from Quarto 1.** Keep the overlay/textbox placement, grouping-by-document with `collapse-after`, crumbs, keyboard shortcuts, copy button, `?q=` target-page highlighting (Quarto 1's highlighter, which handles tabsets; not `pagefind-highlight.js`), and the language terms. Replace the Fuse calls with `pagefind.search()` + `result.data()`, using Pagefind's excerpts for snippets. Whether to keep Algolia Autocomplete as the widget framework is an open question (below).

**Algolia stays opt-in** and ports as-is in a later phase; the UI already abstracts over the two backends in Quarto 1.

## Architecture sketch

```
Pass 2, per page                      post_resources (native only)
┌──────────────────────────┐          ┌──────────────────────────────────┐
│ …RenderHtmlBody          │          │ write_search_index               │
│ SearchCapture ───────────┼─ artifact│  for profile in index.profiles():│
│   (title, lang, crumbs,  │  "search:│    capture = artifacts or        │
│    body HTML / records)  │   <href>"│              .quarto/search cache│
│ ApplyTemplate            │          │    pagefind.add_html_file(...)   │
└──────────────────────────┘          │  files = pagefind.get_files()    │
                                      │  write site_libs/quarto-search/  │
                                      │        pagefind/{meta,index,frag}│
                                      │  write manifest                  │
                                      └──────────────────────────────────┘
page assets: js:search:* (pagefind.js, quarto-search.js[, autocomplete]),
             quarto-search.scss layer, <script id="quarto-search-options">
```

1. **Config.** `resolve_website_search(meta) -> Option<SearchOptions>` mirroring Quarto 1's resolution (bool/object, defaults for location/type by navbar presence). Per-page `search: false` and drafts honoured. Sidebar `search` parsed (closes bd-6cme).
2. **Capture.** A stage or Finalization-tail transform stores, per page, a path-less project-scoped artifact (llms pattern). *What* it stores is open question 1: the rendered body HTML (pre-template, so no nav chrome) wrapped with `lang`, title, and crumbs, or AST-derived per-section plaintext records.
3. **Emit.** `write_search_index` in `WebsiteProjectType::post_resources` (native only): feed every page to `PagefindIndex`, write `get_files()` via `runtime`, persist per-page captures + a manifest under `.quarto/search/` so `Subset` renders rebuild the full index (Pagefind has no incremental mode, but a full rebuild is ~1 ms/page).
4. **Assets.** `js:search:…` artifacts for `pagefind.js` (+ `pagefind-worker.js`) taken from the crate's own `get_files()` output so the runtime version always matches the index, plus our `quarto-search.js`; `quarto-search.scss` as a quarto-sass layer; options JSON in the head via the existing template partials; `<meta name="quarto:offset">` for root-relative URLs.
5. **UI.** Port `quarto-search.js` from Fuse to Pagefind's API; wire `navbar.search`, `sidebar.search`, secondary-nav button, `window.quartoOpenSearch()`.
6. **Compatibility artifact.** Optionally keep emitting `search.json` in Quarto 1's shape (cheap: it is just the records) for users who upload it to Algolia or consume it themselves (open question 4).

## Proposed phases (draft)

- **Phase 0 — Spike in-tree + test plan.** Add `pagefind` (default-features off, cfg-gated to native) to `quarto-core`; measure compile time and `q2` binary delta on macOS/Linux/Windows; confirm `get_files()` + `SystemRuntime` writing; write the failing tests: index records for a fixture site (page + sections + crumbs + `search: false` + drafts), Subset-render manifest behaviour, navbar/sidebar HTML, options JSON.
- **Phase 1 — Config + capture + emit.** `website.search` resolution; capture transform; `write_search_index`; `.quarto/search/` manifest; diagnostics for misconfiguration.
- **Phase 2 — Assets + UI.** Vendored runtime via artifacts; `quarto-search.js` port (overlay + textbox, grouping, collapse-after, crumbs/`show-item-context`, keyboard shortcut, copy button, `?q=` highlighting, tabset opening); SCSS layer incl. dark mode; language terms; sidebar search (bd-6cme).
- **Phase 3 — Books, preview, incremental.** Book projects; `q2 preview --static` reload interaction; hub/WASM behaviour per open question 3.
- **Phase 4 — Algolia opt-in.** Port `algolia.*` options and the insights/consent wiring.
- **Phase 5 — Docs + migration notes.** `website.search` reference; differences from Quarto 1 (no fuzzy, CSP, `file://`); `docs/errors/` pages.

## Open design questions for the user

1. **What do we capture per page: rendered body HTML, or AST-derived records?** Rendered body HTML (captured after `RenderHtmlBody`, before the template) is closest to Quarto 1 (indexes what the reader sees, including engine output, tables, callouts) and lets Pagefind's own HTML parser do heading anchors and element weighting; cost is a second HTML pass over the body in `lol_html`. AST records (plaintext per section via pampa's plaintext writer) are engine-independent and reusable by hub-client Phase B, but the plaintext writer currently drops tables and definition lists, and we would synthesize HTML for Pagefind anyway. **Recommendation: rendered body HTML.**
2. **Keep Algolia Autocomplete as the widget framework, or write a lean custom overlay?** Keeping it is the fastest faithful port (same DOM, same SCSS, same a11y behaviour) at ~120 KB of vendored JS; a custom overlay is smaller and ours to maintain but reopens every UI detail. **Recommendation: keep Autocomplete for the port, revisit later.**
3. **What should the browser preview (hub-client, default `q2 preview`) show?** Options: (a) no search box in the preview (simplest; the box only works in rendered/published output); (b) a profile-only index (titles, headings, descriptions) built from `ProjectIndex` in WASM with MiniSearch, which is already in hub-client; (c) full-text via the existing hub-client source search. **Recommendation: (a) for the port, with (b) as a follow-up strand.**
4. **Keep emitting `search.json`?** It is documented in Quarto 1 as uploadable to Algolia and some users read it. **Recommendation: yes, behind the same resolver, and say so in docs.**
5. **Index location.** Pagefind's convention is `/pagefind/`; q2's is `site_libs/<lib>/`. The runtime locates the index relative to its own script URL, so `site_libs/quarto-search/pagefind/` works. Any reason to prefer the Pagefind convention (third-party tooling)?
6. **Is a future WASM full-site render a requirement?** If yes, we should budget the fork/own-writer route (C1) now rather than later; if no, cfg-gating is enough.
7. **Fuzzy matching.** Accept Pagefind's lack of typo tolerance, or layer a small title-only fuzzy pass (uFuzzy-class) in the UI for the "did you mean" case?

## Risks / tradeoffs (draft)

- **Pagefind format and runtime are version-coupled and undocumented.** Mitigated by taking both from one crate version and vendoring the runtime from `get_files()`; upgrading the crate upgrades both together.
- **Single-maintainer upstream** (CloudCannon/bglw) with steady 2023–2026 releases. If it stalls, the fallback is MiniSearch with a Rust-written serialized index, which the capture/emit split keeps cheap to swap.
- **Binary size / compile time** of the new dependency tree; measured in Phase 0 before committing.
- **Incremental renders** need the same manifest discipline as llms.txt; stale-anchor bugs from Quarto 1 should not be reproduced.
- **CSP** (`wasm-unsafe-eval`, `worker-src blob:`) is a new requirement for some hosts; document and expose `noWorker`.
- **No search in the hub preview** until question 3's follow-up lands; set expectations in the hub UI.
