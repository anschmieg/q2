# M1 project-preview dependency correctness — direct research

Baseline: upstream quarto-dev/q2 commit d110cf7e47d37f9b9771ba5cc7d92a1855934d4a; changes are on fork anschmieg/q2, branch research/q2-m1-preview-deps-direct. No changes to either repository's default branch.

## Verified targeted CI

- Run [38735589185](https://github.com/anschmieg/q2/actions/runs/38735589185) at 2f0d4cbf194a1a2ff715d280e5f076e1f114272e: Rust project-mode unit tests 5 passed, 0 failed, 134 filtered; SPA dependency filter tests 10 passed and SPA typecheck passed. Rust HTTP integration and full browser E2E were **not run**.
- Run [38737709018](https://github.com/anschmieg/q2/actions/runs/38737709018) at ad98409047996965efe6bbbeef1fdf722e17d8db: Rust targeted lib tests, Rust real HTTP endpoint integration, SPA filter + request-race tests, SPA typecheck all passed. These are scoped jobs, not whole-repository native/WASM test verification. Inspect logs for precise per-suite totals and filtered tests.

## What changed

The project-mode endpoint now reuses Q2's ParseDocumentStage + IncludeExpansionStage and DocumentAst::recorded_includes for transitive includes instead of treating the syntax-only single-hop collector as authoritative. It returns {deps:null} (unknown) on incomplete or unsafe analysis; the SPA accepts all edits in that state. For complete closures, the SPA filters unrelated .qmd/.md edits, invalidates and re-fetches on relevant edits and file-list changes, and refuses stale async responses. Writable/shared sessions disable disk-based selective filtering while the synced VFS can differ.

## Follow-up verification: actual browser

Added q2-preview-spa/e2e/project-transitive-deps.spec.ts and a separate browser-transitive-e2e CI job. It exercises the fresh embedded WASM + SPA through the real native q2 preview server in Chromium. The test asserts that changes in index.qmd -> outer.qmd -> leaf.qmd update the active page, an unrelated sibling edit does not trigger a render, and removing/reintroducing the nested dependency updates filtering correctly. **Do not mark these tests passing until their actual CI run completes.**

## Explicit limitations and gates

- The first RED browser test against the *unmodified* upstream baseline has **not been executed**; the upstream deps.rs contract explicitly documents single-hop behavior, but that is source evidence, not a recorded failing E2E test.
- A green targeted test run is not full Rust workspace, WASM, Chromium, or cross-platform coverage.
- The new browser E2E tests and native integration need a completed Actions run; inspect failures before claiming success.
- Clean-full-vs-incremental Q2 output artifact byte comparison and stage timing/selectivity benchmarks remain required for full M1 acceptance. Q2 browser AST/React preview output is not native serialized HTML.
- Test resource dependencies (bibliography/CSL, SVG, Sass) conservatively before claiming comprehensive dependency coverage.
- The native collector is disk-based and deliberately fail-open for writable/shared sessions; a future VFS-aware collector may improve selectivity without stale output.
