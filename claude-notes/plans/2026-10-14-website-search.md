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
**Status (2026-10-14, second pass):** the seven design questions are settled (§ Design decisions); a new federated-search requirement was added (§ Federated search across sites) and the wasm32 fork question was sized (§ wasm32 fork feasibility). **Do not start implementation until the user gives the go-ahead.**

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
- **child: bd-d88bj5r1** (replace Algolia Autocomplete with a Quarto-owned overlay) — filed at design time per decision 2; blocked on the port landing.
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

**UI: Quarto's own, ported from Quarto 1.** Keep the overlay/textbox placement, grouping-by-document with `collapse-after`, crumbs, keyboard shortcuts, copy button, `?q=` target-page highlighting (Quarto 1's highlighter, which handles tabsets; not `pagefind-highlight.js`), and the language terms. Replace the Fuse calls with `pagefind.search()` + `result.data()`, using Pagefind's excerpts for snippets. Algolia Autocomplete stays as the widget framework for the port (decision 2); its replacement is bd-d88bj5r1.

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

- **Phase 0 — Spike in-tree + test plan.** Add `pagefind` (default-features off, cfg-gated to native) to `quarto-core`; measure compile time and `q2` binary delta on macOS/Linux/Windows; confirm `get_files()` + `SystemRuntime` writing (the wasm32 fork feasibility check is already done at design time, § wasm32 fork feasibility, and is not repeated here); write the failing tests: index records for a fixture site (page + sections + crumbs + `search: false` + drafts), Subset-render manifest behaviour, navbar/sidebar HTML, options JSON.
- **Phase 1 — Config + capture + emit.** `website.search` resolution; capture transform; `write_search_index`; `.quarto/search/` manifest; diagnostics for misconfiguration.
- **Phase 2 — Assets + UI.** Vendored runtime via artifacts; `quarto-search.js` port (overlay + textbox, grouping, collapse-after, crumbs/`show-item-context`, keyboard shortcut, copy button, `?q=` highlighting, tabset opening); SCSS layer incl. dark mode; language terms; sidebar search (bd-6cme). Federation-readiness items from § Federated search: one construction site for the Pagefind instance with explicit `basePath`/`baseUrl`; a per-result site-context slot in the result renderer; `manifest.json` next to the bundle.
- **Phase 3 — Books, preview, incremental.** Book projects; `q2 preview --static` reload interaction (and `Access-Control-Allow-Origin: *` on `site_libs/quarto-search/` so a federated parent can be developed against previewed children); no search box in hub-client preview formats (decision 3).
- **Phase 4 — Algolia opt-in.** Port `algolia.*` options and the insights/consent wiring.
- **Phase 5 — Docs + migration notes.** `website.search` reference; differences from Quarto 1 (no fuzzy, CSP, `file://`); `search.json` compatibility note; `docs/errors/` pages.
- **Later, separate strands:** bd-d88bj5r1 (own overlay, a11y); federated search (§ below; strand to be filed once the config sketch is agreed); profile-only search in the hub-client preview (optional, unfiled).

## Design decisions (settled with the user, 2026-10-14)

The seven questions from the first draft are answered; the answers are now constraints on the implementation.

1. **Capture rendered body HTML** (after `RenderHtmlBody`, before the template), wrapped with `lang`, title, and crumbs, and let Pagefind's own HTML parser do heading anchors and element weighting. AST-derived records are not pursued.
2. **Keep Algolia Autocomplete for the port, and replace it afterwards.** Autocomplete has needed accessibility workarounds in Quarto 1, so the replacement is a real goal, not a maybe. Filed at design time as **bd-d88bj5r1** (child of this epic, blocked on the port). The port should therefore keep the Autocomplete-specific code in one module so the swap is contained.
3. **No search box in hub-client preview formats.** `q2 preview --static` goes through `q2 render` and gets search for free; the default hub-client preview renders only the active page and simply omits the box. A profile-only MiniSearch index for the preview stays a possible later strand; nothing is filed.
4. **Keep emitting `search.json`** in Quarto 1's record shape, behind the same resolver, documented as an Algolia-upload / third-party artifact.
5. **Index location is `site_libs/quarto-search/pagefind/`.** Third-party Pagefind tooling that hardcodes `/pagefind/` is limited to the Pagefind CLI's own `--serve`, the stock Pagefind UI's default `bundlePath`, and the runtime's *default* `baseUrl` derivation (it strips a trailing `pagefind/` from the bundle path; for our path it would compute `…/site_libs/quarto-search/` as the site root, which is wrong). None of that matters for a q2 site, with one consequence: **q2's UI must always pass `basePath` and `baseUrl` explicitly**, never rely on the defaults. For federation (below) the fixed path means a parent only needs a child's site URL.
6. **A wasm32 full-site render is nice-to-have, not a requirement.** Context from the user: full-*project* WASM renders are wanted so book previews can emit PDF through the typst wasm renderer, and an "export website .zip" from hub-client is a plausible future; neither needs the Pagefind writer today. Decision: cfg-gate the dependency now, do *not* fork, but establish with a check-only experiment that a fork would work (§ wasm32 fork feasibility).
7. **Accept Pagefind's lack of typo tolerance.** No fuzzy layer in the port; document the difference from Quarto 1.

## Federated search across sites (new requirement, design only)

**The need.** Posit's technical writers want an umbrella documentation site (docs.posit.co, eventually a q2 website) whose search box also returns hits from roughly a dozen child sites (docs.posit.co/posit-team/, docs.posit.co/ide/server-pro/, docs.posit.co/connect/, …), each of which will also be a q2 website. This is an advanced feature, not for the first release, and the question for now is only whether the recommended engine and the architecture above leave room for it.

**Pagefind supports exactly this, natively.** It is documented as "multisite search" (`pagefind.app/docs/multisite/`): after `pagefind.init()`, the page calls `pagefind.mergeIndex(bundlePath, options)` once per extra site, and subsequent `pagefind.search()` calls return one merged result set. Verified in the 1.5.2 runtime the crate embeds (`vendor/pagefind_public_search_api.1.5.2.js`):

- Each merged index becomes a second `PagefindInstance` with its own `basePath` (an absolute cross-origin URL is fine; the runtime only strips the origin when it matches `window.location.origin`), its own `pagefind-entry.json`, meta, index-chunk and fragment fetches, and its own `indexWeight`, `mergeFilter`, `baseUrl`, `language`, `ranking`, `excerptLength` options.
- `search()` fans out with `Promise.all` over every instance, concatenates results, re-sorts by `score × indexWeight`, and merges filter counts; `preload()` fans out the same way. The worker-mode wrapper (`pagefind_worker.js`) implements the same `mergeIndex`.
- Result URLs are built by `fullUrl(raw)`: `baseUrl + "/" + raw` unless `raw` is already absolute. So a parent passes `baseUrl: "https://docs.posit.co/connect/"` per child and q2 keeps writing root-relative record URLs (as Quarto 1 does), which keeps each child bundle relocatable.
- Per-document metadata (title, crumbs, section headings) travels in each child's fragments, so Quarto's grouping-by-document and sub-results work unchanged for merged hits.

**Cost model.** For every query, each child costs one `pagefind-entry.json` (cached after the first), its meta file once, about one 33 KB index chunk per new query term, and a 2 KB fragment per displayed result; all children are fetched in parallel. With the quarto.org numbers from the spike that is roughly 150 KB per child on the first query and 33 KB per child afterwards, so a dozen children means on the order of 1.8 MB on the first query and 400 KB per later query. That is the "admittedly inefficient" the writers already accepted, it is still far below Quarto 1's single 591 KB gzip download per site, and `preload()` on focus hides most of the latency. It is fine for a dozen children and would not be fine for a hundred.

**Constraints that shape the design now.**

1. **One Pagefind format version across the federation.** Merged instances share the *parent's* wasm module: `this.backend` is the module-global `wasm_bindgen` glue, and `mergeIndex` initialises children with `load_wasm: false`. A version mismatch between a child's `pagefind-entry.json` and the parent's runtime only produces a `console.warn`; an actual format change would fail at decode time, silently. Pagefind's format is undocumented and versioned only by release. Loading a per-child wasm is *not* a one-line patch (the wasm-bindgen glue is a singleton), so the practical rule is: all sites in a federation are rendered by q2 releases that embed the same `pagefind` crate version. Design response: q2 writes a `manifest.json` next to the bundle (q2 version, pagefind version, site title, default language, bundle path), and the parent UI reads it before merging and skips any child whose pagefind version differs, reporting that in the console and, in preview, as a diagnostic. Pinning one crate version per q2 release, which the single-crate recommendation already implies, makes this a non-issue for sites that upgrade together.
2. **Same-origin versus cross-origin.** The motivating case (docs.posit.co with children under sub-paths) is *same-origin*, so no CORS configuration is needed at all. Children on other origins must serve the bundle directory with `Access-Control-Allow-Origin` (q2 cannot set response headers; the docs must say so, and the parent's CSP `connect-src` must list the child origins). `q2 preview --static` should add `Access-Control-Allow-Origin: *` on `site_libs/quarto-search/` so a federated parent can be developed locally against previewed children on other ports.
3. **Bundle discoverability.** With decision 5 the bundle path is fixed, so for a q2 child the parent only needs the child's site URL; for a non-q2 Pagefind site, an explicit bundle path override.
4. **Language.** A child in another language is searched with the parent's stemmer (Pagefind's documented limitation). Acceptable; note it in docs.
5. **Showing where a hit comes from.** Results must say which site they belong to. Pagefind's `mergeFilter` can tag every result from a child (e.g. `site: "Posit Connect"`) without the child tagging its pages, and the result renderer needs a per-result site-context slot (empty for local hits). Cross-site ranking is only adjustable through `indexWeight`; BM25 scores from different corpora are not strictly comparable, which is inherent and acceptable.
6. **Algolia path is unaffected**; federation is Pagefind-only (an Algolia user federates through Algolia's own multi-index search).

**Config sketch (not final; to be agreed before a strand is filed):**

```yaml
website:
  search:
    federate:
      - url: https://docs.posit.co/connect/
        title: Posit Connect        # shown as result context; defaults to the child manifest's title
        weight: 0.8                 # Pagefind indexWeight, default 1
      - url: https://docs.posit.co/ide/server-pro/
      - url: https://example.org/other-pagefind-site/
        bundle: pagefind/           # override for a non-q2 Pagefind site
```

**What the port does now so this stays cheap later** (all folded into Phase 2 / Phase 3 above):

- Construct the Pagefind instance in exactly one place in `quarto-search.js`, always with explicit `basePath` and `baseUrl`, so adding `mergeIndex` calls is additive.
- Emit `site_libs/quarto-search/manifest.json` (q2 version, pagefind version, title, language, bundle path).
- Keep record URLs root-relative; never bake the site URL into the index.
- Give the result renderer a site-context field per result and per group.
- Keep the `--static` preview CORS header as a small Phase 3 item.

**Verdict:** federated search is possible with Pagefind with modest implementation effort (mostly UI and config, no engine work), and the architecture in this plan does not have to change to allow it. The one real risk is the format-version coupling across sites, which the manifest check and q2's single-crate pinning contain. A strand will be filed once the config sketch is agreed.

## wasm32 fork feasibility (decision 6, check-only)

The question is how far the `pagefind` 1.5.2 crate's *indexing* core is from compiling for `wasm32-unknown-unknown`, so that a future "full-project render in the browser" can be budgeted with confidence. Read of the crate source (5.7k lines in `src/`):

- **Core that must survive unchanged:** `fossick/` (HTML parsing via `lol_html`, splitting, stemming: 2.4k lines), `index/` (word/filter/meta encoding with `minicbor`: 0.9k), `fragments/`, `output/entry.rs`. Their imports are pure: `lol_html`, `pagefind_stem`, `regex`, `unicode-*`, `hashbrown`, `minicbor`, `serde`, `emojis`, `flate2`. Nothing platform-specific.
- **Native-only touch points, all at the edges:** `rayon` `into_par_iter` at four sites (`lib.rs` fossick-many, `index/mod.rs` page processing, filter encoding, chunk encoding), trivially sequentialised; `tokio::fs` + `tokio::time::sleep` in `output/mod.rs`, used only by the write-to-disk path (the in-memory `write_files_to_memory` path, which `get_files()` uses, does not need them); `tokio::task::spawn_blocking` + `tokio::sync::Mutex` in `service/api.rs` (the library API), replaceable by direct calls; `std::fs::File::open` in `fossick/mod.rs` only for the file-based `add_html_file` path (the string-based path is already in-memory); `std::thread::spawn` in `service/mod.rs` (the Node-service transport), `serve.rs`, `runner.rs`, `main.rs` are CLI-only and would be cfg-gated out.
- **Dependency tree:** `tokio` with `rt-multi-thread`/`fs`/`time` is the main blocker (tokio compiles on wasm32 with only `sync`/`macros`/`io-util`/`rt`); `rayon` must go; `actix-*`/`portpicker` are already behind the `serve` feature; `charabia` (CJK, `extended` feature) is optional; `console`, `twelf` (CLI config), `wax` (globs), `async-compression` are suspects to be confirmed by the check below. `include_dir!` of `vendor/wasm` (33 language runtimes) compiles anywhere but carries its size into any binary that embeds the crate.

A check-only experiment (patched copy in the scratchpad, `cargo check --lib --no-default-features --target wasm32-unknown-unknown`, no fork committed anywhere) is being run to turn this read into a measured patch size; results are appended below when available.

## Risks / tradeoffs (draft)

- **Pagefind format and runtime are version-coupled and undocumented.** Mitigated by taking both from one crate version and vendoring the runtime from `get_files()`; upgrading the crate upgrades both together.
- **Single-maintainer upstream** (CloudCannon/bglw) with steady 2023–2026 releases. If it stalls, the fallback is MiniSearch with a Rust-written serialized index, which the capture/emit split keeps cheap to swap.
- **Binary size / compile time** of the new dependency tree; measured in Phase 0 before committing.
- **Incremental renders** need the same manifest discipline as llms.txt; stale-anchor bugs from Quarto 1 should not be reproduced.
- **CSP** (`wasm-unsafe-eval`, `worker-src blob:`) is a new requirement for some hosts; document and expose `noWorker`.
- **No search in the hub preview** (decision 3); set expectations in the hub UI.
- **Federated search couples sites to one Pagefind format version.** The parent's wasm decodes every child's chunks; a mismatch only logs a warning in stock Pagefind. Mitigated by the bundle `manifest.json` (the parent skips and reports children with a different format version) and by pinning one `pagefind` crate version per q2 release; see § Federated search across sites.
