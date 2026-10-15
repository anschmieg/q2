# Q2 M1 preview dependency collector — paired performance experiment (2026-10-15)

This is a **research result**, not part of the proposed core-first PR. Benchmarked native `GET /api/preview/deps?page=index.qmd` on GitHub Actions on one Linux runner. All binaries were compiled with the same pinned Rust toolchain/build profile, then measured sequentially with 3 warmup requests per case and 8–32 samples. These are warm endpoint latencies, **not** document render times or CPU profiles.

## Three implementations

- **Upstream baseline:** `d110cf7e47d37f9b9771ba5cc7d92a1855934d4a` — direct single-hop include extraction.
- **Original M1:** research commit `7ea131b2dbfdde43a034a90b7a0420214b90913a` — full transitive closure using existing parser/expander, but `ProjectContext::discover(&root)` re-enumerates every project file on each endpoint request.
- **Optimized M1:** research branch `research/q2-m1-perf-optimized`, code commit `43059bbba5107b0cead19ea77b6447ab72674e1c` — same transitive semantics, `ProjectContext::discover(&abs_page)` to avoid project-wide file discovery.

## Measured median endpoint latency

| Fixture | Upstream | Original M1 | Optimized M1 | Original / optimized |
|---|---:|---:|---:|---:|
| No includes | 0.520 ms | 2.463 ms | 2.351 ms | 1.05× |
| 1 include | 0.526 ms | 2.716 ms | 2.556 ms | 1.06× |
| 8 nested includes | 0.511 ms | 4.496 ms | 4.411 ms | 1.02× |
| 64 nested includes | 0.630 ms | 21.084 ms | 20.188 ms | 1.04× |
| 8 includes + 100 unrelated pages | 0.572 ms | 6.212 ms | 4.601 ms | 1.35× |
| 8 includes + 1,000 unrelated pages | 0.611 ms | 11.978 ms | 4.772 ms | 2.51× |
| 8 includes + 3,000 unrelated pages | 0.579 ms | 37.646 ms | 5.602 ms | 6.72× |

Evidence: [three-way run #38789100191](https://github.com/anschmieg/q2/actions/runs/38789100191), **SUCCESS**, with optimized Rust `project_mode` unit tests **5 passed**, HTTP endpoint test **1 passed**, and benchmark raw samples in artifact `q2-m1-native-deps-overhead`.

Independent original-vs-upstream paired run [#38788568356](https://github.com/anschmieg/q2/actions/runs/38788568356) also passed, with 3,000 unrelated pages **0.670 vs 18.224 ms**; numbers differ substantially between CI runs due to runner conditions, so **do not combine absolute numbers across runs**.

## Interpretation / architecture decision

Discovering from the active page instead of the project root materially improves large-project behavior (6.72× in the 3,000-sibling case) and is a simple performance fix; its Rust unit and HTTP tests pass. It **does not** address the inherent repeated parse/expand overhead on deep include chains (20.188 ms vs 0.630 ms for depth 64). The original M1 preview implementation should not be submitted without performance reconsideration.

The user has opted for **core-first** dependency tracking instead. Q2's `DocumentProfile.includes` is already a transitive, content-hashed side channel from the core include expansion stage; indexing those files **once** in `ProjectDependencyGraph` allows constant-time reverse lookups and per-document edge updates, without rediscovering the project or reparsing includes per request. This is the subject of separate branch `research/q2-core-file-dependencies`. No changes from this preview experiment have been pushed to upstream or to either default branch.

The latency data are single-run measurements in a fixed order; they do not establish a general regression factor under varying host load, browser/render environment, or cache state. No full Chromium E2E was rerun on the optimized preview experiment; its verification is limited to the stated Rust checks and endpoint microbenchmark.
