# M1 project-preview dependency correctness — verified research

**Status: first scoped M1 slice accepted (2026-10-15).** See [final acceptance report](2026-10-15-q2-m1-first-slice-acceptance.md) for exact results, source hashes, skipped tests, timing limitations, and research boundaries.

## Source and implementation

- Pinned upstream `quarto-dev/q2` `d110cf7e47d37f9b9771ba5cc7d92a1855934d4a`.
- Work was performed only in fork `anschmieg/q2` on research branch `research/q2-m1-preview-deps-direct`; no default-branch, Quartier, deployment or upstream-PR changes.
- `crates/quarto-preview/src/deps.rs` now derives transitive included-file closure from real `ParseDocumentStage` / `IncludeExpansionStage` / `DocumentAst::recorded_includes`. It returns null when complete coverage cannot be proven.
- The SPA filters unrelated edits only for complete dependency sets, refreshes on relevant content and membership changes, rejects out-of-order fetch responses and stays fail-open for unknown closures. Disk-based selectivity is disabled for writable/shared VFS sessions.

## Verification history

- [38735589185](https://github.com/anschmieg/q2/actions/runs/38735589185): initial focused unit/type checks passed; HTTP integration/browser tests were not part of that run.
- [38737709018](https://github.com/anschmieg/q2/actions/runs/38737709018): Rust project dependency unit/HTTP tests and SPA checks passed.
- [38739195904](https://github.com/anschmieg/q2/actions/runs/38739195904): two real Chromium nested-include E2E tests passed; SPA default Vitest inadvertently *excluded* component integration tests.
- [38752138281](https://github.com/anschmieg/q2/actions/runs/38752138281): jsdom integration tests explicitly enabled and **42 passed**; targeted Rust/SPA/Chromium also green.
- **[38767256307](https://github.com/anschmieg/q2/actions/runs/38767256307)** at `aafdd3d590ccdd243d0370c80d7d2620cc921ba2`: all four jobs green; **16,080 native Rust tests passed, 202 skipped**, plus 13 SPA unit, 42 jsdom integration, 2 real Chromium E2E, SPA typecheck; targeted Rust 5 unit/1 HTTP integration passed (subset of the full native suite). Native HTML SHA-256 parity succeeded for targeted/clean/warm output after nested include edit; unrelated sibling output SHA and mtime unchanged. Actual timing observations: 334 ms cold full, 34 ms targeted, 38 ms clean full final, 29 ms warm targeted; **not** a controlled speedup comparison.

## Deliberate omissions

No baseline RED browser run against untouched upstream (source contract identifies the single-hop defect); no whole-output-tree/PDF byte parity or general CPU speedup estimate; no all-platform browser matrix; no exhaustive resource channels; no VFS-native selective invalidation. These are documented limitations rather than silently treated as passes. The first slice satisfies its narrowed acceptance criterion; broader Q2-M1 candidates such as Pass-2 resumption are separate work.

No further CI retrigger is necessary for documentation-only sign-off. Tested implementation SHA and CI evidence remain linked above.
