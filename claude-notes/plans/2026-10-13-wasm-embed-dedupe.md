# Dedupe the embedded `wasm_quarto_hub_client_bg.wasm` in the q2 binary

## Overview

PR #452 added a second embedded copy of the ~35.7 MB render WASM: the
preview SPA carries one inside `quarto-preview`\'s `viewer-embed.tar.zst`,
and `quarto-mcp-launcher`\'s `include_dir!` of `ts-packages/quarto-hub-mcp/dist-bundle/`
carries another, raw. This plan dedupes them to a single embedded copy
(v2 plan backlog item 4, `claude-notes/plans/2026-07-28-hub-mcp-get-errors-v2.md`;
flagged as a follow-up in the PR #452 description).

Work lands as commits on `pr-452-fixes`. **Do not push** — the user
reviews before this branch goes anywhere.

Design decisions:

1. **New leaf crate `crates/quarto-wasm-embed`** holds the WASM once,
   zstd-19-compressed at build time, lazily decompressed into a
   `OnceLock` on first use. Source scan order: exactly-one
   `q2-preview-spa/dist/assets/wasm_quarto_hub_client_bg-*.wasm`, else
   `ts-packages/quarto-hub-mcp/dist-bundle/wasm_quarto_hub_client_bg.wasm`,
   else placeholder (empty). Sourcing from the SPA dist keeps the
   preview's served bytes identical to what Vite content-hashed, by
   construction; the dist-bundle fallback keeps a bundle-only dev flow
   (`build-hub-mcp-bundle` without the SPA) working.
2. **Preview strips the WASM from the viewer archive** (build.rs,
   exactly-one rule, same as the shared crate) and records its
   dist-relative path in `QUARTO_PREVIEW_SHARED_WASM_REL`;
   `lookup_embedded` falls back to the shared crate for exactly that
   path. Everything (gzip regen, editor fallback, join-frontend
   manifest hits, `embedded_gz`) flows through `lookup_embedded`, so
   one branch covers all serving. Manifests are file-based
   (`spa-manifest.json` written by the npm build into the dist, riding
   the archive) and stay byte-identical — the served bytes at the
   recorded path are identical by construction.
3. **Launcher strips the WASM from its `include_dir!`** (build.rs
   writes a filtered copy of dist-bundle in `OUT_DIR`) and appends the
   shared bytes to the extraction set under the fixed sibling name
   `wasm-host.mjs` reads (`new URL('./wasm_quarto_hub_client_bg.wasm',
   import.meta.url)`). The content hash covers the extraction set, so
   cache keys still track WASM identity. Strip is conditional on the
   bundle's `build-info.json` carrying a `wasmSha256` field (new in
   this change): pre-change stale bundles keep their own embedded
   WASM — self-consistent, no unguarded mismatch state. At runtime the
   launcher appends shared bytes iff the embed has no WASM of its own,
   and warns (stderr) when the shared bytes\' SHA-256 differs from
   `wasmSha256` (dev-time staleness tripwire; never breaks a launch).
4. **`bundle.mjs` records `wasmSha256`** in `build-info.json`. The
   standalone tarball / future npx channel keeps the WASM inside
   dist-bundle — only the q2 embed strips it.
5. **Release-pipeline latent gap fixed alongside**: PR #452's
   `buildWasmHost` refuses the stub on the bundle path, but the
   per-target `build` legs and the `hub-mcp-bundle` job run
   `npm run bundle` without `hub-client/wasm-quarto-hub-client/`
   (only `web-payloads` builds the WASM, and it does not upload the
   wasm-pack dir). Merging PR #452 as-is breaks nightly/release.
   Fix: `web-payloads` uploads `hub-client/wasm-quarto-hub-client/`;
   the `hub-mcp-bundle` job gains `needs: web-payloads` + a download
   step (the `build` legs already download the whole artifact).

Expected binary delta: one zstd copy (~9.5 MB) replaces the raw 35.7 MB
launcher copy and the viewer tar's WASM share (~9.5 MB compressed) —
roughly −35 MB per q2 binary, times every release target.

## Checklist

### Phase 1 — shared crate
- [ ] `crates/quarto-wasm-embed`: build.rs (scan, compress, rustc-env) + lib.rs (`WASM_FILENAME`, `wasm_bytes() -> Option<&'static [u8]>`)
- [ ] Tests: WASM magic bytes when present; both-tree-states behavior
- [ ] Check `claude-notes/instructions/windows-dev.md` for exclusion needs

### Phase 2 — preview
- [ ] build.rs: exactly-one hashed-WASM scan, archive strip, `QUARTO_PREVIEW_SHARED_WASM_REL`
- [ ] `lookup_embedded` shared-WASM fallback
- [ ] Unit tests: fallback serves shared bytes; stale hash misses; editor-mode resolves
- [ ] Integration test: manifest `.wasm` entry served identity + gzip, sha matches manifest
- [ ] `cargo nextest run -p quarto-preview`

### Phase 3 — launcher
- [ ] build.rs: filtered embed copy; strip iff `build-info.json` has `wasmSha256`
- [ ] `run()`: append shared WASM when the embed lacks it; bail (actionable) if shared is None; stderr warn on sha mismatch
- [ ] `--launcher-info` reports the WASM mode (`shared <sha>` / `embedded-in-bundle` / `MISSING`)
- [ ] Unit tests: extraction-set assembly, sha parse, mismatch detection
- [ ] `cargo nextest run -p quarto-mcp-launcher`

### Phase 4 — TS bundle
- [ ] `bundle.mjs`: `wasmSha256` in `build-info.json`
- [ ] `bundle.test.ts`: pin the field against the real `.wasm`
- [ ] `npm test -w ts-packages/quarto-hub-mcp`

### Phase 5 — release pipeline
- [ ] `web-payloads` uploads `hub-client/wasm-quarto-hub-client/`
- [ ] `hub-mcp-bundle` job: `needs: web-payloads` + download step

### Phase 6 — docs
- [ ] AGENTS.md: update the two staleness sections (preview + mcp) for the shared embed
- [ ] v2 plan: check off backlog item 4
- [ ] Point the pr-452-fixes plan at this plan

### Phase 7 — verification
- [ ] `cargo build --workspace` + `cargo nextest run --workspace`
- [ ] `cargo xtask lint`
- [ ] Rebuild dist-bundle + `cargo build --bin q2`
- [ ] E2E launcher: fresh `QUARTO_MCP_CACHE_DIR`, `q2 mcp --help`, cache dir holds the WASM with the SPA-dist sha; `wasm-host.mjs` init under plain Node
- [ ] E2E preview: integration test green; real-server fetch of the WASM path
- [ ] Binary size before/after
- [ ] `cargo xtask verify --skip-hub-build` (Rust-only surface; quarto-preview is not a hub-client WASM dep)

## Notes

- All three on-disk copies verified byte-identical before starting
  (sha256 `a24e2111…c2d96de`: SPA dist, dist-bundle, hub-client
  wasm-pack dir).
- The editor embed (`--ui editor`) already dedupes byte-identical
  files against the viewer dist at build time; after this change its
  WASM fallback resolves through `lookup_embedded` to the shared
  crate. No editor changes.
- `q2 provide-hub` delegates `auth-stream.mjs` and never touches the
  WASM. Its auth bridge (`quarto-hub-provider/src/token_bridge.rs`)
  extracts `embedded_files()` directly, so it intentionally keeps its
  own files-only cache dir (hash covers no WASM): the auth bridge
  never pays the 36 MB extraction, and stays available even in the
  (build-broken) no-shared-WASM state. Deliberate, not an oversight.
- All verification below ran on this tree (real dists): unit +
  integration suites green; `q2 mcp --launcher-info` reports
  `render-wasm: shared (35743971 bytes, sha256 a24e2111df3d)`; a fresh
  `QUARTO_MCP_CACHE_DIR` extraction holds the WASM next to
  `wasm-host.mjs` with the bundle-stamped sha; `ensureInit()` under
  plain Node loads the extracted host and answers VFS calls.
- Measured embed deltas (debug build): viewer tar.zst 11.74 MB →
  4.09 MB; launcher embed drops the raw 35.74 MB WASM; the shared
  crate adds 7.69 MB zstd. Net ≈ −35.7 MB per q2 binary.
- Real-binary E2E (2026-10-13): `q2 preview` on a fixture project
  serves `assets/wasm_quarto_hub_client_bg-DqJ6YX1g.wasm` at 200 /
  `application/wasm` / 35,743,971 bytes / sha256 `a24e2111…` (the
  shared embed\'s bytes; output inspected). `q2 mcp` extraction +
  plain-Node `ensureInit()` recorded above.
- Environment findings while verifying: two knitr visibility tests
  failed until the stale local knitr 1.52.4 (dev build) was replaced
  with CRAN knitr 1.52 — unrelated to this change. `typst_caption_image`
  and `fetch_blocking_gives_up_on_a_stalled_connection` each failed
  once under full-suite parallel load and pass standalone (flaky,
  pre-existing).
- Drive-by fix folded into this work: `local-render.test.ts` had a
  TS2352 under the package `tsc` build (pre-existing branch breakage
  from fcf189f3f); cast now goes through `unknown`.
- Pre-existing qmd errors in this branch\'s plan files (unescaped
  apostrophes, `_site` emphasis) fixed; `q2 render --strict
  claude-notes` is green again.
