# Q2-M1 — scoped first-slice acceptance (verified)

**Status: ACCEPTED on 2026-10-15, scoped to project-mode live-preview transitive dependency correctness.** This is a research-branch acceptance, **not** an upstream merge, default-branch release, or completion of every possible Q2-M1 optimization. No upstream PR, production deployment, or Quartier application change was performed.

- Fork: [anschmieg/q2](https://github.com/anschmieg/q2)
- Branch: `research/q2-m1-preview-deps-direct`
- Upstream source pin: `d110cf7e47d37f9b9771ba5cc7d92a1855934d4a` (Q2 0.33.0-nightly.20261013).
- **Code revision tested:** `aafdd3d590ccdd243d0370c80d7d2620cc921ba2`.
- **Verified acceptance run:** [GitHub Actions #38767256307](https://github.com/anschmieg/q2/actions/runs/38767256307), concluded **success** 2026-10-15 16:36 UTC. All four jobs completed successfully.
- The later **documentation-only** sign-off commit does not alter tested code. GitHub Actions was intentionally skipped for that documentation-only commit.

## Measured acceptance results

| Check | Actual result | Evidence / limitation |
|---|---|---|
| Full native Rust workspace | **16,080 passed; 0 failed; 202 skipped** | `native-full-suite` job, `cargo nextest run --tests --cargo-profile ci --no-fail-fast`; 240.929 s measured test execution. Skipped tests are **not** counted as passed. |
| Rust project dependency unit tests | **5 passed, 0 failed; 134 filtered out** | `rust-deps` job, targeted `cargo test -p quarto-preview --lib project_mode`. Included in the full run; not additive to the 16,080 total. |
| Real Rust HTTP dependency endpoint | **1 passed, 0 failed; 61 filtered out** | `rust-deps` job, `boot::project_deps_http_endpoint_includes_nested_children`. Included in full native suite; not additive. |
| SPA unit tests | **13 passed** | `spa-deps`, source filtering/decoding/membership tests. |
| SPA jsdom component integration | **42 passed** | `spa-deps`, explicit `npm run test:integration --workspace=q2-preview-spa -- src/PreviewApp.integration.test.tsx`. This addresses the original default-Vitest exclusion. |
| SPA TypeScript | **Passed** | `npm run typecheck --workspace=q2-preview-spa`. |
| Real Chromium preview E2E | **2 passed, 0 failed** | `browser-transitive-e2e` job; fresh WASM, trace-viewer, SPA, and native `q2` built from this commit. Both completed in approximately 9 seconds combined. |
| Native HTML exact-byte oracle | **Passed** | `scripts/ci/q2-m1-native-parity.sh` in the Chromium job, comparing SHA-256 of **`index.html`** on a deterministic non-executable nested-include website. |
| Selective output write | **Passed** | During targeted render, unrelated `sibling.html` SHA-256 **and nanosecond-precision mtime** remained unchanged. |
| Browser selectivity | **Passed** | Changing unrelated `sibling.qmd` produced no extra `__renderTicks` during the E2E observation; editing transitive `leaf.qmd` changed visible iframe content. |

### Raw native HTML oracle

The deterministic fixture used `index.qmd → outer.qmd → leaf.qmd`, plus unrelated `sibling.qmd`. A nested leaf edit changed the rendered active page. Subsequently, targeted, clean full, and warm targeted renders of the **same final source state** yielded **identical raw bytes**:

```text
initial_full_sha256       1df2ac998f536a300a85e3059f66e4c565c372e7bd723159557fcb3a9aa12cf1
targeted_final_sha256     46cd9f3b642ead7d8874b27ce59dd3d305697f4d4bbade409d1efe049279af9d
clean_full_final_sha256   46cd9f3b642ead7d8874b27ce59dd3d305697f4d4bbade409d1efe049279af9d
warm_targeted_sha256      46cd9f3b642ead7d8874b27ce59dd3d305697f4d4bbade409d1efe049279af9d
unchanged_sibling_sha256  ef1ff4c172ae0f01a101fe97fc2e896eacbe74eb5b2477ca8391e738f2af95cd
```

Measured wall times from **one** CI-host run: initial cold full **334 ms**, targeted after nested edit **34 ms**, clean full final **38 ms**, warm targeted **29 ms**. These are descriptive timings, **not a controlled speedup experiment**; cold caches, different render sets, and measurement noise prevent a general runtime claim. The reliable selectivity signal is the unchanged sibling output; the reliable correctness signal is raw-byte equality for the tested HTML artifact.

Logs/artifacts: [acceptance run](https://github.com/anschmieg/q2/actions/runs/38767256307) — `q2-m1-native-parity` (including per-mode `QUARTO_PERF_STATS=1` logs and `timing.tsv`) and `q2-m1-full-native-results`.

## Implemented behavior

1. The project-preview dependency endpoint uses the existing Rust `ParseDocumentStage`, `IncludeExpansionStage`, and `DocumentAst::recorded_includes` to obtain *transitive* includes, rather than presenting a single-hop AST syntax walk as complete.
2. Missing, cyclic, unsafe, symlink-aliased, changing-during-analysis, and unparseable dependencies become **unknown** (`{"deps":null}`); the SPA fails open and accepts all source edits in that state, avoiding stale-output suppression.
3. The SPA invalidates and reacquires dependency closures after relevant source or membership changes, discards out-of-order HTTP results, and avoids refresh for unrelated source edits with a complete closure.
4. Writable/shared VFS sessions conservatively disable disk-backed selective filtering, because VFS state may be ahead of local disk.
5. Chromium E2E verifies the *real* preview UI for nested leaf updates, unrelated sibling filtering, and remove/re-add of an include; it does not treat React/AST output as equivalent to native HTML serialization.

## Boundaries and work deliberately deferred

| Area | What is established | What is **not** established |
|---|---|---|
| Source include invalidation | Recursive QMD/MD includes in tested project-mode cases | All extreme concurrent edits, every filesystem and Windows junction variant |
| Browser output | Fresh WASM/SPA build and two real Chromium tests | Full browser matrix (Firefox/Safari), exhaustive visual/source-map parity |
| Native output | One deterministic HTML artifact exactly equal between targeted, clean-full and warm render | Whole-site output-tree byte equality, volatile sitemap/assets, PDF/Typst, Q1→Q2 byte parity, side-effectful Lua or execution engines |
| File resources | Non-source updates continue broadly invalidating preview | Precise transitive dependency closure for SVG/images, bibliography/CSL, Sass/CSS, themes, crossrefs |
| Collaborative editing | Safe broad invalidation when editable/shared VFS may differ from disk | VFS-aware *selective* dependency tracking |
| Performance | Sibling render avoided in fixture; four wall-time observations | Statistical benchmarks, calibrated CPU speedup, generic incremental-fragment cost savings |
| Historical baseline | Pinned upstream code deliberately documents single-hop project mode | **Baseline RED Chromium E2E was not run**. The pre-patch failure is supported by source contract, not a recorded failing E2E. |

The 2026-10-15 upstream `main` at `e812742d1ee4a2c31b6ce87b82a357f8752f4de8` still documented single-hop project dependencies. **Recheck upstream drift before preparing a small upstream PR**, and ideally reproduce the baseline RED case then; no such PR has been created or authorized.

### Exit decision

**First Q2-M1 implementation slice: ACCEPTED.** The observed correctness defect is fixed with end-to-end evidence, selective avoidance of unrelated renders, exact native HTML parity in a defined deterministic scope, a complete passing native Rust run with explicit skipped count, and a reproducible CI recipe.

This sign-off does **not** mean all previously listed optional Q2-M1 candidate patches have been implemented. Pass-1→Pass-2 checkpoint resumption, proven-pure stage reuse, VFS-aware selective closure, and authoritative HTML fragment rendering remain separate research/implementation work and should not be retroactively included in this patch.
