# Q2-M1.1 — Adversarial review and performance baseline

**Scope:** research branch only. Preserve review/core histories. No renderer cache, no navigation graph semantic changes, no upstream PR.

## Verified design and adversarial findings

- The input closure is built from the same completed core Pass-1 index used by the render. A missing profile, unrepresentable dependency path, or sidebar diagnostic is treated as unknown. The SPA rejects malformed/absent snapshots and fails open.
- The synced file inventory previously checked only path membership. Replacing a file at the same path with a new Automerge document identity could leave a valid-looking but obsolete dependency snapshot. Inventory invalidation now detects identity changes.
- A completed asynchronous render's state updater can be applied after a newer edit/navigation. The successful AST branch already guarded by revision; M1.1 applies the same guard to unsupported-format, error-response, and thrown-error branches.
- New tests cover same-path replacement, harmless reordering, render failure → fail-open → recovery, and real Chromium single-file nested-include invalidation. Existing tests cover project-mode transitive includes, include removal/readdition, sidebar links, and unrelated edits.
- Every source edit that passes the filter causes a whole-document render. The graph does not justify element-level invalidation or cached Pass-1 resumption.
- Dynamic Lua/engine source reads and symlink/alias identities are not fully recorded; the non-source signal path remains conservative. Investigate these explicitly before upstream submission.

## Informational benchmark

`crates/quarto-core/tests/live_invalidation_perf.rs` measures graph build and closure on synthetic, **already-computed** Pass-1 profiles. Cases: 100/1000/3000 project documents with 0/1/8/64 transitive includes; sidebars of 32/128/256/512 members; and 1000 listing documents with 1/32 content globs. Each reports the median of five debug/native Rust iterations. CI prints `q2_m1_perf` lines in the `rust-deps` job; elapsed-time thresholds are intentionally **not** used as assertions.

This is a reproducible core-only baseline, not an end-to-end speedup claim: it does not measure WASM serialization, project profile extraction, network/sync, React updates, or iframe paint. CI runner load can change absolute timings. Large sidebar complete-subgraph expansion is O(S²); comparative baseline evidence should determine whether changing it is worthwhile.

## Milestone gate

Rust graph and WASM adapter tests, SPA unit/integration tests, format/type checks, and real Chromium project- and single-file E2E must pass. Capture measured CI output before claiming acceptance. Next M2 requires comparative edit-to-paint and allocation benchmarks and a correct Pass-1 checkpoint reuse contract; do not optimize speculative hot paths.
