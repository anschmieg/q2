# Q2 M1 first-slice acceptance evidence

Scope: fork anschmieg/q2, branch research/q2-m1-preview-deps-direct, upstream source pin d110cf7e47d37f9b9771ba5cc7d92a1855934d4a. No default-branch or Quartier changes. This is the project-preview dependency-correctness slice, not Pass-2 checkpoint resumption or fragment caching.

## Already verified
- GitHub run 38752138281 (SHA 209ce50f28845d4feda9e0b49c5edf3f66015c12): 5 Rust project-mode unit tests, 1 Rust HTTP test, 13 SPA unit, 42 jsdom integration, 2 real Chromium E2Es and SPA typecheck all passed. Other suites were not claimed passing by that targeted run.
- Real Chromium tests verify that index.qmd -> outer.qmd -> leaf.qmd updates on nested leaf edits, no re-render for unrelated sibling, and correct removal/readdition invalidation. They use fresh embedded WASM and SPA against the real q2 server.
- Upstream main inspected Oct 15 at SHA e812742d1ee4a2c31b6ce87b82a357f8752f4de8. Production project-preview deps.rs still documents single-hop include behavior.

## Acceptance verification added (not yet executed at time of writing)
- scripts/ci/q2-m1-native-parity.sh: a deterministic nonexecuting native website with nested includes and an unrelated sibling. Initial full HTML -> nested-leaf edit -> targeted index render -> clean full at the same final source state -> warm targeted. Compares exact SHA-256 bytes of index.html, proves edit changed output, and verifies unrelated sibling hash and mtime unchanged during targeted render. Also records wall milliseconds and QUARTO_PERF_STATS logs. Only one HTML output artifact is compared: this does not prove broad byte identity for PDF, sitemap, or dynamic Lua.
- Full patched native Rust suite with QUARTO_CI=1 and pinned Deno 2.9.0, Pandoc 3.11, Typst 0.15.1, Rust nightly 2026-04-28, using --no-fail-fast. Await actual counts/logs before marking passed.
- Real Chromium E2E already asserts no extra render tick for unrelated changes (avoided work) and a positive tick/content update for nested leaf changes. This is a bounded selectivity observation, not a calibrated cross-machine CPU speedup.

## Boundary matrix
| Channel | Verified/safe handling | Outside this slice |
|---|---|---|
| Transitive source includes, remove/re-add | Rust/HTTP/Chromium | Extreme multi-peer interleavings |
| Async fetch races, membership | jsdom component tests | Arbitrary network scheduling |
| Parse failure, cycle, absent/outside project, symlinks | Conservative null/unknown fail-open | Exhaustive platform-specific symlink/junction matrix |
| SVG, binary images, CSS, bibliography, CSL, Sass, theme | Broad invalidation of non-source changes remains | Selective transitive closure per asset class |
| Writable/shared VFS-ahead-of-disk | Disk-based selective filtering disabled | VFS-native collector |
| Native authoritative output | New SHA-256 oracle, pending | Q1/Q2 byte parity, PDF/Typst, nondeterministic effects |
| Historical RED baseline | Upstream code documents single hop | Original baseline E2E not executed; do not claim measured RED |

First-slice M1 signoff requires actual green native parity and full patched Rust suite in addition to the existing targeted tests. Any failed, filtered, skipped or unrun tests remain explicitly identified. No PR to upstream has been requested.
