# Pagefind-as-a-library spike (bd-eip0naf4, 2026-10-14)

Scratch crate (`Cargo.toml` + `main.rs` here; build it outside the workspace)
that feeds Quarto 1 `search.json` records into the `pagefind` 1.5.2 crate's
library API (`pagefind::api::PagefindIndex`) and measures what it emits.
Two modes: `html` synthesizes one HTML page per document (h1 title, h2 per
section with its anchor id) and calls `add_html_file`; `custom` joins the
sections into plain text and calls `add_custom_record`.

```
cargo build --release
./target/release/pf-spike search.json <out-dir> html|custom
```

## Build findings

- `pagefind = { version = "1.5.2", default-features = false }` (drops the
  actix `serve` feature) **does not compile on its own**: `output/mod.rs`
  uses `tokio::io::AsyncWriteExt` but the crate only gets tokio's `io-util`
  transitively through actix. The consumer must enable it:
  `tokio = { features = ["io-util", "fs", "rt-multi-thread"] }`. Worth an
  upstream PR.
- The crate pulls `rayon`, `tokio` (fs, rt-multi-thread), `lol_html`,
  `minicbor`, `flate2`, `unicode-segmentation`, and `include_dir!`-embeds
  the prebuilt `pagefind_web` wasm + all UI JS. Spike binary: 15.3 MB
  release. (The survey verified separately that it fails to build for
  `wasm32-unknown-unknown`.)
- `get_files()` returns every output file in memory (`SyntheticFile
  {filename, contents}`), so the writer can go through `SystemRuntime`
  instead of `std::fs`.

## Measurements (Apple Silicon laptop, release build)

quarto.org (`search.json`: 241 pages, 1738 sections, 2.54 MB raw / 591 KB gz):

| mode | add | build | index chunks | fragments | meta | wasm |
|---|---:|---:|---|---|---:|---:|
| html | 188 ms | 36 ms | 26 files, 820 KB total (min 19.6 KB, median 32.6 KB, max 35.8 KB) | 241 files, 709 KB (median 2.2 KB, max 13.3 KB) | 2.4 KB | 68 KB |
| custom | 197 ms | 33 ms | 25 files, 757 KB | 241 files, 681 KB | 2.4 KB | 68 KB |

r4ds (38 pages, 254 sections, 977 KB raw / 275 KB gz): 12 chunks / 395 KB,
38 fragments / 292 KB, add 84 ms, build 17 ms.

All `.pf_*` files are already gzip -9 (the runtime inflates them itself).
`pagefind.js` is 45.5 KB raw (~13 KB gz). The other ~500 KB of emitted files
are Pagefind's three bundled UIs, which Quarto would not ship.

**Per-query transfer, quarto.org:** `pagefind.js` (~13 KB gz) + wasm (68 KB)
+ meta (2.4 KB) + 1–3 chunks (~33 KB each) + one fragment per displayed
result (~2 KB) ≈ 120–160 KB for the first query, ~33 KB per new chunk after
that — versus 591 KB gz up front for Quarto 1, then zero. The crossover is
around one or two queries per visit on a site this size; on a 5,000-page
site Pagefind's per-query cost stays roughly constant while Quarto 1's
up-front cost scales with the corpus (~12 MB gz extrapolated).

Caveats: language came out as `unknown` because the synthetic HTML had no
`lang` attribute, so no stemmer was selected (the real integration will
pass the document `lang`). Section text here was Quarto 1's already
extracted text, not real rendered HTML, so element weighting beyond h1/h2
was not exercised.
