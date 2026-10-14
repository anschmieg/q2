# Client-side full-text search for static sites — state of the art, October 2026 (bd-eip0naf4)

Web survey run 2026-10-14. Candidate engines for replacing Quarto 1's Fuse.js
`search.json`, evaluated against:

- **C1** index buildable from pure Rust on `wasm32-unknown-unknown` (or at least cfg-gatable out of the WASM tree)
- **C2** wire size / chunking at 1k–5k pages (20k–100k sections)
- **C3** relevance and features (ranking, prefix, fuzzy, stemming, CJK, snippets)
- **C4** runtime size, license, maintenance
- **C5** API suitable for a custom UI (we keep Quarto's own overlay)

Registry/GitHub figures were pulled on 2026-10-14 via the npm and crates.io
APIs and `gh api`; everything else cites a URL. Items that could not be
confirmed are marked **unverified**.

---

## 1. Fuse.js (baseline)

- **Status:** v7.5.0, published 2026-07-13; Apache-2.0; repo active (pushed 2026-08-09), 20.5k stars ([releases](https://github.com/krisk/Fuse/releases)).
- **C1:** Trivial. Fuse has no real index: `Fuse.createIndex(keys, list)` produces `{keys, records}` holding normalized copies of the field strings, and `Fuse.parseIndex(json)` reloads it ([docs](https://fusejs.io/api/indexing.html)). Quarto 1's `search.json` *is* effectively the index.
- **C2:** Monolithic, and its size is the corpus size, because the "index" stores the text. No chunking.
- **C3:** Bitap approximate string matching, scanned linearly over every record per query. No inverted index, no BM25/TF-IDF, no stemming, no tokenization (CJK "works" only as substring matching). Weighted keys and `extendedSearch` exist. Independent benchmarks put it at the bottom for latency: uFuzzy's author measured 33,875 ms for 86 queries over 162k phrases vs 434 ms for uFuzzy and 1,453 ms for MiniSearch, with 245 MB peak heap ([uFuzzy README](https://github.com/leeoniya/uFuzzy)); FlexSearch's README table also ranks `fuse` last. One team reported Fuse "way too slow and memory hungry" and moved to MiniSearch ([acreom](https://acreom.com/blog/the-quest-for-a-great-search)).
- **C4:** ~400 KB unpacked, small gz bundle, zero deps. Healthy maintenance.
- **C5:** Plain JS API; this is what Quarto's UI already wraps.
- **Verdict:** Fine for a few hundred records; the wrong primitive for 20k–100k sections.

## 2. Pagefind

- **Status:** v1.5.2 (2026-04-12) on npm and crates.io; MIT; repo pushed 2026-10-08; 5.5k stars; 133 open issues+PRs. Cadence: 1.1.0 (BM25-aligned ranking), 1.2.0 (Python bindings), 1.3.0 ("stabilized the Rust library"), 1.4.0, 1.5.0 (2026-04-06), 1.5.2 ([releases](https://github.com/CloudCannon/pagefind/releases)). 1.5.0 added: search in a Web Worker, index chunks ~45% smaller via delta encoding, CJK *query* segmentation via `Intl.Segmenter`, metadata/title search with configurable weights, diacritics handling, Polish/Estonian stemming, a web-components "Component UI" ([v1.5.0 notes](https://github.com/CloudCannon/pagefind/releases/tag/v1.5.0)).

**C1 — the `pagefind` crate as a library and on wasm32.** The crate is usable as a library: `pagefind::api::PagefindIndex::new(Option<PagefindServiceConfig>)`, then `async fn add_html_file(source_path, url, content)`, `add_custom_record(url, content, language, meta, filters, sort)`, `add_directory(path, glob)`, `build_indexes()`, `write_files(output_path)` and `get_files() -> Vec<SyntheticFile>` (in-memory path+bytes) ([docs.rs](https://docs.rs/pagefind/latest/pagefind/api/struct.PagefindIndex.html)). Features: `default = ["serve"]` (actix-web/actix-files/portpicker), `extended = ["dep:charabia"]` (CJK segmentation) ([docs.rs features](https://docs.rs/crate/pagefind/latest/features)). The Node and Python packages just spawn the native binary and talk to it over a pipe ([Node API](https://pagefind.app/docs/node-api/), [Python API](https://pagefind.app/docs/py-api/)).

**It does not compile for `wasm32-unknown-unknown`.** Verified with a scratch crate (`pagefind = { version = "1.5.2", default-features = false }`, the project's `nightly-2026-04-28` toolchain): `cargo check --target wasm32-unknown-unknown` fails inside tokio with *"Only features sync,macros,io-util,rt,time are supported on wasm"* — pagefind enables `tokio` with `fs` and `rt-multi-thread` (confirmed via `cargo tree -e features`). Other non-wasm dependencies: `rayon` (parallel HTML parsing in `lib.rs`, parallel chunk encoding in `index/mod.rs`), `async-compression`, and `include_dir!` embedding the prebuilt per-language `pagefind_web` wasm + `pagefind.js` from `vendor/wasm` ([Cargo.toml](https://github.com/CloudCannon/pagefind/blob/main/pagefind/Cargo.toml)). A grep of the core sources (~3.4k lines across `lib.rs`, `runner.rs`, `output/mod.rs`, `fossick/mod.rs`, `index/mod.rs`, `service/api.rs`) shows the native surface is bounded: `tokio::fs` in `output/mod.rs` (3 sites) and `fossick/mod.rs` (1), rayon in `lib.rs` (4) and `index/mod.rs` (4), `tokio::spawn`/runtime in `service/api.rs` (4). A fork that swaps `tokio::fs` for in-memory writes, rayon for sequential iterators, and keeps `minicbor`/`flate2`/`lol_html`/`unicode-segmentation` is plausible — but it is a fork of a 0.0.0-versioned workspace with no upstream wasm32 CI (no issue or PR about wasm32 indexing found). `charabia` (extended/CJK) on wasm32: **unverified**.

**Index format (read from source, not documented as a spec).**
- `pagefind-entry.json`: `{version, languages: {lang: {hash, wasm, page_count}}, include_characters}` (live example: `https://pagefind.app/pagefind/pagefind-entry.json`).
- `pagefind.{hash}.pf_meta`: CBOR (`minicbor`) `MetaIndex {version, pages:[{hash, word_count}], index_chunks:[{from, to, hash}], filters, sorts, meta_fields}` (`index/index_metadata.rs`).
- `index/{hash}.pf_index`: CBOR `WordIndex { words: [PackedWord { word, pages: [PackedPage { page_number (delta), locs: [i32 delta], meta_locs }], additional_variants }] }` (`index/index_words.rs`). Chunks are alphabetical word ranges (`from`/`to`), cut every 20,000 words by default (`PAGEFIND_UNSTABLE_INDEX_CHUNK_SIZE`; source comment: "TODO: Parameterize these chunk sizes via byte size rather than word count") (`options.rs`, `index/mod.rs`).
- `fragment/{hash}.pf_fragment`: JSON `{url, content, word_count, filters, meta, anchors:[{element, id, text, title, location}]}` (`fragments/mod.rs`).
- `wasm.{lang}.pagefind`: gzipped wasm-bindgen module. Every file is gzip `Compression::best()` from `flate2`; the JS runtime inflates with its own `gz.js`, so hosting needs no compression support (`output/mod.rs`, `pagefind_web_js/lib/coupled_search.ts`).
- Precedent that the format is reimplementable: **Pagefind.Net** (nullean, MIT) is a pure-.NET writer of `pagefind-entry.json`/`.pf_meta`/`.pf_index`/`.pf_fragment` pinned to "Pagefind 1.5.2" (`PagefindTargetVersion`); Elastic's docs-builder adopted it to drop the subprocess ([repo](https://github.com/nullean/pagefind-net), [NuGet](https://www.nuget.org/packages/Pagefind.Net), [elastic/docs-builder#3709](https://github.com/elastic/docs-builder/pull/3709)). It also shows the cost: the format is version-pinned and undocumented, so a reimplementation must track `pagefind_web` releases.

**C2 — measured.** pagefind.app (48 pages, v1.5.2): `pagefind.js` 45.5 KB raw / 13.4 KB gz; `wasm.en-us.pagefind` 72 KB (pre-gzipped, includes the English Snowball stemmer); `.pf_meta` 523 B. apache.org (1,033 pages, still on v1.3.0): 491 index chunks totalling ~33 MB on disk (typically 40–80 KB, max 400 KB each) and 939 fragments ~21 MB — but a query loads only the 1–3 chunks whose alphabetical range covers its terms plus one fragment per displayed result. Demo deployments: mdn.pagefind.app indexes 14,242 pages on v1.5.0; xkcd 3,000; godot 1,579. Vendor claims: "10,000 page site … under 300 kB total, including the Pagefind library", "closer to 100 kB" typical ([pagefind.app](https://pagefind.app/), [intro post](https://cloudcannon.com/blog/introducing-pagefind/)). Upper bound anecdote: one Hugo user crashed indexing at ~300k pages and got 250k at best ([Hugo forum](https://discourse.gohugo.io/t/scaling-pagefind-to-over-1-million-pages/56109)). For 1k–5k pages this is comfortably inside the design envelope.

**C3.** Ranking "moved to a BM25-aligned algorithm" in 1.1.0 with tunables `termFrequency` (1.0), `termSimilarity` (1.0), `pageLength` (0.75), `termSaturation` (1.4), `diacriticSimilarity` (0.8), `metaWeights` (title 5.0) ([ranking docs](https://pagefind.app/docs/ranking/)). Prefix matching is inherent ("part" matches "party" and "partition"; `termSimilarity` ranks the closer length higher). Element weights h1=7 … h6=2, body 1, `data-pagefind-weight` 0–10, quadratic scale ([weighting](https://pagefind.app/docs/weighting/)). Stemming: Snowball via `pagefind_stem` for ~33 languages (feature list in `pagefind_web/Cargo.toml`); none for CJK. CJK: index-time segmentation only in the `extended` build (charabia) — the non-extended binary logs a warning for `zh`/`ja` (`lib.rs`); query-time `Intl.Segmenter` since 1.5 ([multilingual](https://pagefind.app/docs/multilingual/)). **No fuzzy/typo tolerance** (not in docs; **unverified** whether planned). Excerpts with `<mark>`, `plain_excerpt`, per-heading `sub_results` with anchor URLs, `matchedMetaFields`.

**C4.** Runtime: 13 KB gz JS + ~72 KB wasm per language, zero external deps (wasm-bindgen glue is inlined). CSP needs `script-src 'wasm-unsafe-eval'` and `worker-src blob:` or `noWorker: true` ([hosting](https://pagefind.app/docs/hosting/)). MIT; one primary maintainer (bglw) under CloudCannon; steady release cadence 2023–2026.

**C5.** The JS API is UI-agnostic: `options()`, `init()`, `search(term, {filters, sort})`, `debouncedSearch(term, opts, 300)`, `preload(term)`, `search(null, {filters})` for filter-only, compound filters `any/all/not/none`, `filters()` counts, `mergeIndex`/`indexWeight`/`mergeFilter` for multi-site, `destroy()`; results are `{id, data()}` so metadata/excerpts are fetched lazily; highlighting on the target page via `pagefind-highlight.js` + `highlightParam` ([API](https://pagefind.app/docs/api/), [filtering](https://pagefind.app/docs/js-api-filtering/), [multisite](https://pagefind.app/docs/multisite/), [highlighting](https://pagefind.app/docs/highlighting/)). Starlight, Nextra and bloggers build custom UIs on it ([example](https://dee.underscore.world/blog/building-pagefind-ui)). Grouping-by-document comes for free: one result per page with `sub_results` per heading.

## 3. MiniSearch

- **Status:** v7.2.0 (2025-09-16); MIT; zero deps; 6.2k stars; repo last push 2025-09-16 (13 months quiet, but no open-issue pile-up: 15) ([repo](https://github.com/lucaong/minisearch)).
- **C1:** Strong. `toJSON()`/`loadJSON()`/`loadJSONAsync()` (async, chunk-friendly parse) serialize the inverted index; the v4 format change was made explicitly "to abstract away the internal implementation details … the new format is simpler, facilitating the job of tools that create a serialized MiniSearch index in other languages" ([CHANGELOG](https://github.com/lucaong/minisearch/blob/master/CHANGELOG.md)). A Rust writer is therefore a sanctioned path. Alternatively build at runtime from a JSON corpus (Quarto-1 style); indexing is "at least 4× faster" since v4.
- **C2:** Monolithic by design; no built-in sharding. Index ≈ corpus-sized JSON (stores terms→doc→field frequencies plus stored fields). For 20k–100k sections expect multi-MB; the only mitigation is building several independent instances and merging, which breaks global BM25 normalization.
- **C3:** BM25+ (tunable `bm25` option), prefix, fuzzy (edit distance via Hanov's Levenshtein-trie method), per-field `boost`, `combineWith`, `processTerm` for stemming/stop-words, custom `tokenize` (VitePress plugs `Intl.Segmenter` for zh/ja), `autoSuggest`, `match` info for highlighting ([docs](https://lucaong.github.io/minisearch/)). No snippets — you keep the text yourself.
- **C4:** Small (exact gz figure **unverified**; ~7 KB gz per the hub-client plan), zero deps, well documented. Used by VitePress and Jupyter Book 2 (myst-theme `packages/search-minisearch`), and already by q2's hub-client editor search.
- **C5:** Plain synchronous JS API; easiest fit for Quarto's existing overlay.

## 4. FlexSearch

- **Status:** 0.8.212 on npm (2025-09-06); GitHub release 0.8.2 (2025-05-21); repo pushed 2026-06-28; Apache-2.0; 13.8k stars ([repo](https://github.com/nextapps-de/flexsearch)).
- **C1:** Weak. `index.export(cb)` emits several `(key, data)` pairs, but "You need to import every key! Otherwise, your index does not work"; the per-key payload format is undocumented; Worker and persistent indexes "do not support Import/Export"; fast-boot `serialize()` is experimental ([export-import.md](https://github.com/nextapps-de/flexsearch/blob/master/doc/export-import.md)). A Rust writer would have to reverse-engineer internal `Map` layouts that change between versions.
- **C2:** Monolithic in the browser (persistent IndexedDB/SQLite backends exist, but that is a different deployment model).
- **C3:** Fast contextual/positional scoring (not BM25; **unverified** details), Document index with multi-field and tags, Resolver (and/or/xor/not, boost), highlighting, encoders incl. a CJK preset and Latin phonetic presets, Worker support. The README's benchmark table is self-reported and unit-less.
- **C4:** 16.3 KB gz full bundle / 11.4 compact / 4.5 light. Single-maintainer; 0.8 was a big rewrite.
- **C5:** OK, but the document/resolver API is idiosyncratic. Used by Rspress and Hugo's Hextra; Nextra dropped it for Pagefind in v4.

## 5. Lunr.js / Elasticlunr

- **Status:** lunr 2.3.9 (2020-08-19), MIT, repo pushed 2024-07, 130 open issues; elasticlunr 0.9.5 (2016-09-08), repo pushed 2022-12. Both maintenance-dead on the JS side.
- **C1:** The *Rust* side is alive: `elasticlunr-rs` 3.1.0 (2026-07-26) builds elasticlunr-compatible JSON indexes and is what mdBook 0.5.4 (`crates/mdbook-html`, feature `search`) and Zola use ([crates.io](https://crates.io/crates/elasticlunr-rs)). wasm32: **unverified** (pure-Rust deps, probably yes).
- **C2:** Monolithic and large: Material for MkDocs measured 335 KB (78 KB gz) for its own docs and 4.4 MB (1.2 MB gz) for an 800k-word corpus even after a 42–47% reduction ([blog](https://squidfunk.github.io/mkdocs-material/blog/2021/09/13/search-better-faster-smaller/)); VS Code's team saw ~10 MB lunr indexes for ~3 MB of Markdown ([docfind post](https://code.visualstudio.com/blogs/2026/01/15/docfind)). mdBook mitigates by loading the index only when the search box opens (0.4.52).
- **C3:** TF-IDF, wildcard prefix, field boosts, Porter stemmer; `lunr-languages` adds other stemmers (CJK via TinySegmenter for ja; zh depends on node-only jieba — **unverified**).
- **Verdict:** Only interesting as proof that a Rust-side writer of a JS engine's index is a well-trodden path.

## 6. Orama

- **Status:** `@orama/orama` 3.1.18 (2025-12-19), Apache-2.0, repo pushed 2026-10-08, 10.6k stars. The company's focus has moved to **OramaCore** (Rust, AGPL-3.0) and Orama Cloud; the JS library is still maintained but had no release in the last ten months ([repo](https://github.com/oramasearch/orama)).
- **C1:** `@orama/plugin-data-persistence` exports/imports whole databases (JSON/binary); format not documented as a stable contract (**unverified**). Monolithic.
- **C2/C3:** BM25, typo tolerance, stemming "in 30 languages", facets, geosearch, vector/hybrid; CJK via `@orama/tokenizers` (experimental). Fumadocs uses it by default and warns that static mode "requires clients to download the exported search indexes … for large docs sites it can be expensive" ([Fumadocs](https://fumadocs.dev/docs/headless/search/orama)).
- **C4:** "<2 kB" is marketing; the full package is 2.2 MB unpacked. Risk: open-source library is now the on-ramp for a hosted product.

## 7. tinysearch

- **Status:** 0.11.1 (2026-09-14), MIT/Apache-2.0, 3k stars, actively maintained. v0.11 added an exact inverted index with prefix search (≥3 chars) as default; the Xor8 index remains for the smallest artifact ([releases](https://github.com/tinysearch/tinysearch/releases)).
- **C1:** The index is **compiled into the wasm module** — `tinysearch -m wasm` runs `cargo build` at site-build time, so you need a Rust toolchain wherever you build the site ([README](https://github.com/tinysearch/tinysearch)). Non-starter.
- **C2/C3:** ~2 KB/article uncompressed; monolithic. No documented ranking, no snippets, no stemming. "Small to medium sites."

## 8. Stork

Effectively dead: author posted "I'm winding down my work with Stork" (discussion #360, 2023-06-18); last release 1.6.0 (2023-01-12); last push 2023-07 ([repo](https://github.com/jameslittle230/stork)). Monolithic index (a 500-article test site produced 6 MB / 2 MB gz per [hugo-stork](https://github.com/jmooring/hugo-stork)).

## 9. tantivy → WASM

- tantivy 0.26.2 (2026-09), MIT, very active — but the maintainers closed "Does tantivy purposed to run in browser?" (Mar 2026) with "out of scope"; threading is the blocker ([#2855](https://github.com/quickwit-oss/tantivy/issues/2855)); the wasm RFC [#541](https://github.com/quickwit-oss/tantivy/issues/541) (2019) and [#1413](https://github.com/quickwit-oss/tantivy/issues/1413) (2022) remain open.
- **Summa** (izihawa) 2.0.0 (2026-09-25), MIT, 297 stars, has `summa-wasm` with `RemoteIndex` over HTTP range requests, `IpfsIndex`, `LocalIndex` persisted to OPFS/IndexedDB ([README](https://github.com/izihawa/summa/blob/master/summa-wasm/README.md)); based on a tantivy fork (**unverified**); wasm size undocumented; build needs wasm-pack + LLVM + Node 22. Niche and heavy.

## 10. SQLite FTS5 over HTTP range requests

- Building blocks: `sql.js-httpvfs` (phiresky, Apache-2.0, last push 2024-08), `sqlite-wasm-http` (ISC, "experimental", last release 2023-12), `wa-sqlite` 1.1.2 (2026-08, MIT, active). FTS5 traversals need read-ahead tuning and 1 KB pages ([sqlite-wasm-http](https://github.com/mmomtchev/sqlite-wasm-http)).
- Static-site search products on top are dormant: BlogSearch (last push 2023), webidx, sphinx-sqlite3fts.
- **C1:** Producing an FTS5 database from Rust on wasm32 means compiling SQLite to wasm inside the renderer (**unverified**). Range requests depend on hosting (some CDNs/proxies strip `Range`). Not recommended.

## 11. uFuzzy / fuzzysort

uFuzzy 1.0.19 (2025-08), MIT, 7.5 KB, zero index (scans a string haystack; Latin-optimized) ([repo](https://github.com/leeoniya/uFuzzy)); fuzzysort 4.0.2 (2026-08), MIT, ESM-only. Both are list-filtering tools for short strings, not full-text engines. Potentially useful as a title-only "instant" layer on top of a real engine.

## 12. Hosted (opt-in backends)

Algolia relaunched DocSearch on 2025-10-01 with self-service onboarding and AskAI ([announcement](https://www.algolia.com/about/news/algolia-unveils-a-new-era-for-documentation-search-with-self-service-onboarding-and-askai)); plan terms (May 2025) give accepted projects up to 5M records / 50M requests / 5M crawled URLs per month free ([terms](https://www.algolia.com/policies/docsearch-plan-specific-terms)); commercial sites pay (~$49/month/site reported). Typesense maintains `typesense-docsearch-scraper` plus `typesense-docsearch.js` ([guide](https://typesense.org/docs/guide/docsearch.html)). Meilisearch offers `docs-scraper` ([blog](https://www.meilisearch.com/blog/how-to-integrate-a-relevant-search-bar-to-your-documentation)). All three fit the "Quarto renders the HTML, a crawler indexes it" model and need no renderer changes.

## 13. New in 2025–2026

- **docfind (Microsoft, Jan 2026)** — Rust; `docfind_core` (fst with Levenshtein automata, FSST string compression, postcard, optional RAKE) has a `wasm` feature; the CLI patches the index blob into a prebuilt wasm template, so the index is a single `.wasm` (VS Code docs, ~3,700 heading-sections: 5.9 MB / 2.7 MB Brotli; 50k AG News articles: 11.5 MB / 5.2 MB br; ~0.4–3 ms/query). RAKE keyword scores, not BM25; prefix + typo tolerance; license **unverified** (GitHub reports NOASSERTION); v0.5.1 (2025-11-19), repo pushed 2026-10-03; not on crates.io ([post](https://code.visualstudio.com/blogs/2026/01/15/docfind), [repo](https://github.com/microsoft/docfind)). Monolithic and large.
- **StaticSearch (Craig Buckler, mid-2025)** — pure-JS indexer + web component, per-word JSON index files loaded incrementally, 13 KB JS ([Show HN](https://news.ycombinator.com/item?id=44746761)). Young, single maintainer, Node indexer; the per-word-file idea is worth noting.
- **Pagefind 1.5** is the only established engine with a substantive 2026 release.

---

## Comparison table

| Engine | Latest / license | C1: Rust→wasm32 indexer | C2: chunked? | C3: ranking · prefix · fuzzy · stem · CJK · snippets | C4: runtime |
|---|---|---|---|---|---|
| Fuse.js | 7.5.0 (2026-07) · Apache-2.0 | trivial (JSON = corpus) | no | linear Bitap · n/a · yes · no · substring only · no | small, 0 deps |
| **Pagefind** | 1.5.2 (2026-04) · MIT | crate **fails** on wasm32 (tokio fs/rt-multi-thread, rayon); fork or reimplement undocumented CBOR+gzip format (Pagefind.Net precedent) | **yes** (alphabetical word-range chunks + per-page fragments) | BM25-aligned · yes · **no** · Snowball ×33 · extended build + Intl.Segmenter · yes, with `sub_results` | 13 KB gz JS + ~72 KB wasm/lang, 0 deps, Worker |
| MiniSearch | 7.2.0 (2025-09) · MIT | **good** — serialized format intended for other-language writers | no | BM25+ · yes · yes · via `processTerm` · via custom tokenizer · match info only | small, 0 deps |
| FlexSearch | 0.8.212 (2025-09) · Apache-2.0 | poor — undocumented export, all keys required | no (in-browser) | positional/contextual · yes · encoders · presets · CJK preset · highlight | 16 KB gz |
| Lunr / Elasticlunr | 2.3.9 (2020) / 0.9.5 (2016) · MIT | elasticlunr-rs 3.1.0 exists (wasm32 unverified) | no | TF-IDF · wildcard · limited · Porter+langs · partial · no | stale JS |
| Orama | 3.1.18 (2025-12) · Apache-2.0 | persistence plugin, format not a contract | no | BM25 · yes · yes · 30 langs · experimental tokenizers · no | 2.2 MB pkg; vendor pivot |
| tinysearch | 0.11.1 (2026-09) · MIT/Apache | index compiled **into** wasm → needs cargo at build time | no | none documented · ≥3 chars · no · no · no · no | ~80 KB gz incl. index |
| Stork | 1.6.0 (2023-01) · Apache-2.0 | dead | no | — | — |
| tantivy/Summa | 0.26.2 / 2.0.0 · MIT | tantivy: "out of scope"; Summa wasm read-only remote | HTTP range | full · yes · yes · yes · ? · yes | heavy, unverified size |
| SQLite FTS5 + range VFS | wa-sqlite 1.1.2 · MIT | needs SQLite in wasm inside renderer (unverified) | HTTP range | FTS5 BM25 · yes · no · limited · ICU optional · yes | ~1 MB wasm; hosting-dependent |
| uFuzzy / fuzzysort | 2025-08 / 2026-08 · MIT | n/a (no index) | no | fuzzy only | tiny |
| docfind | 0.5.1 (2025-11) · MIT? | plausible (fst/fsst/postcard) but unverified | no (single wasm) | RAKE · yes · yes · no · no · no | MBs |

## What peer SSGs use for local search (2026)

| Generator | Engine | Notes |
|---|---|---|
| Astro Starlight | **Pagefind** (default) | Algolia via official plugin; Typesense community plugin ([docs](https://starlight.astro.build/guides/site-search/)) |
| Nextra 4 | **Pagefind** | replaced FlexSearch (v3); postbuild `pagefind` step ([docs](https://nextra.site/docs/guide/search)) |
| VitePress | **MiniSearch** | defaults `{fuzzy: 0.2, prefix: true, boost: {title: 4, text: 2, titles: 1}}`, `Intl.Segmenter` for zh/ja; Algolia DocSearch option ([docs](https://vitepress.dev/reference/default-theme-search)) |
| Rspress | **FlexSearch** | |
| Docusaurus | Algolia (first-class), Typesense; community local plugins (both lunr) | no official Pagefind plugin ([docs](https://docusaurus.io/docs/search)) |
| Hugo | none built-in | Hextra theme: FlexSearch; many sites run Pagefind post-build |
| Zola | built-in **elasticlunr-rs** index or `fuse_json` | no UI shipped ([docs](https://www.getzola.org/documentation/content/search/)) |
| mdBook 0.5.4 | **elasticlunr-rs** + elasticlunr.js | index lazy-loaded on search open since 0.4.52 |
| Jupyter Book 2 / MyST | **MiniSearch** (`myst-theme/packages/search-minisearch`) | |
| MkDocs Material | **lunr** + lunr-languages in a Web Worker | index 335 KB→4.4 MB depending on corpus |
| Sphinx | custom `searchindex.js` (stemmed terms→doc ids, titles, objects) | summaries fetched from the page HTML on demand |
| rustdoc | custom column-major JSON, VLQ-hex, roaring bitmaps | descriptions in lazily fetched shards; client-side Levenshtein ([dev guide](https://rustc-dev-guide.rust-lang.org/rustdoc-internals/search.html)) |
| Fumadocs | Orama (static JSON mode) | docs warn about full-index download on large sites |
| VS Code docs | docfind | single wasm |
| Quarto 1 | Fuse.js | |

## Benchmarks found

No credible three-way head-to-head exists; what is measurable: (a) Pagefind: 13 KB JS + 72 KB wasm fixed, then 1–3 chunks of 40–80 KB per query at ~1k pages (apache.org, v1.3; v1.5 chunks ~45% smaller) vs. lunr at 1.2 MB gz for 800k words (MkDocs Material) and ~10 MB for 3 MB of Markdown (VS Code); (b) linear scanners (Fuse, uFuzzy, fuzzysort) vs. inverted indexes (MiniSearch, FlexSearch) differ by 10–100× in query time at 10^5 strings ([uFuzzy bench](https://github.com/leeoniya/uFuzzy)); (c) FlexSearch's README table claims large wins but is unit-less and self-published. 2026 write-ups converge on "Pagefind for serious volume, MiniSearch-class for small precise corpora, Fuse only for tiny lists" ([johal.in](https://johal.in/static-site-search-options-guide), [dev.to](https://dev.to/morinaga/static-site-search-for-astro-in-2026-why-i-picked-pagefind-over-algolia-and-lunr-6dg)).

---

## Conclusion — ranking for Quarto 2's constraints

**1. Pagefind's index format + vendored `pagefind.js`/wasm runtime, with a Quarto-driven writer.** The only candidate that satisfies C2 for 20k–100k sections without a server, has the richest C3/C5 surface (BM25-ish, prefix, stemming, CJK, `sub_results` per heading, excerpts, filters, lazy `data()`), and is the de-facto choice of Starlight and Nextra. The C1 cost is real: the `pagefind` crate does **not** build for `wasm32-unknown-unknown` (tokio `fs`/`rt-multi-thread`; rayon; vendored assets), and the on-disk format is undocumented CBOR+gzip, version-pinned to the runtime. Viable routes: (a) use the crate as a native library and cfg-gate it out of wasm32; (b) fork the indexer core (~3.4k lines; ~12 tokio/rayon call sites) into a `no-threads, in-memory` crate; (c) write a Quarto `pf_*` writer directly (what Pagefind.Net did; `minicbor` + `flate2` are wasm-clean). Either way, pin one `pagefind_web` version and add a format-compat test. Missing: typo tolerance.

**2. MiniSearch with a Rust-built serialized index.** Best C1 story (serialization format explicitly meant for foreign writers; `loadJSONAsync`), BM25+, prefix, fuzzy, boosting, custom tokenizer for CJK, MIT, zero deps, simplest fit for Quarto's existing overlay (C5). Weakness is C2: monolithic, corpus-sized index — fine to ~1–2k pages, painful at 5k pages/100k sections unless Quarto adds its own sharding (per-section instances, losing global IDF). Reasonable as the default for small sites or as the fallback engine for the hub preview.

**3. A Quarto-native compact index + small runtime** (rustdoc/docfind style). Maximum control over C1, C2 and C5, at the cost of owning ranking, stemming, CJK segmentation, and a query runtime — essentially re-deriving Pagefind. Worth it only if route 1's format-pinning risk is judged unacceptable.

Not recommended: FlexSearch (undocumented export), Orama (vendor pivot, monolithic), tinysearch (needs cargo at site build), Stork (dead), tantivy/Summa (indexer cannot run in-browser), SQLite FTS5 over range requests, Fuse/uFuzzy/fuzzysort (not full-text). Keep Algolia/Typesense/Meilisearch as opt-in crawler-based backends unchanged.
