# Core-driven live render invalidation — implementation research

**Date:** 2026-10-15. **Status:** implemented on a research branch; the end-to-end verification and performance gate are not yet complete.

## Scope and preservation

- Fork `anschmieg/q2`, branch `research/q2-core-live-invalidation`, branched from the clean core review commit `ccea8e58bf7249dc77bdc5bd3fd09569c89c2f91`, itself based on upstream `e308835bd062495d96424d1c8c2e56a5fe554818`.
- Existing preview research `research/q2-m1-preview-deps-direct`, core research `research/q2-core-file-dependencies`, and clean core-only `review/q2-core-include-dependency-graph` are preserved. No default branch, Quartier, or upstream PR changed.
- The objective is **document-level correctness and selective scheduling** using the core project's existing profiles, not element-level recomputation yet.

## Single source of dependency truth

1. Core `DocumentProfile.includes` already records actual transitive include files while Pass 1 expands includes. Project `DocumentProfile.body_link_targets`, `nav_dependencies`, listing globs and sidebar membership feed existing `ProjectDependencyGraph` edges.
2. Core `ProjectDependencyGraph::render_inputs_for(active_doc)` computes the transitive closure over *both* the existing document/profile graph and file-include edges. Dependents of other linked documents' include files are accounted for too. It does not parse files, access VFS, or schedule work.
3. `ProjectPipeline::last_project_index()` exposes the actual Pass-1 index after the render; a cheap Arc clone avoids recomputing/discovering project profiles.
4. The WASM boundary `live_dependencies::render_input_paths` builds the core graph from the finished index and emits forward-slash, project-relative `render_inputs` in `RenderResponse`. An incomplete Pass-1 index, unresolved/escaping path, or graph diagnostic produces *no* snapshot, meaning unknown.
5. The preview SPA uses `render_inputs` to avoid unnecessary rerenders for unrelated `.qmd`/`.md` edits. It does not fetch the previous native disk-based `/api/preview/deps` endpoint. Non-source updates stay broadcast. Accepted events, membership changes, active-page navigation, capture changes, reconnections, and failed renders invalidate the snapshot and fail open until a new render completes.
6. A revision guard prevents out-of-order render completions from overwriting the current dependency snapshot. All work happens against the synced WASM VFS, not the native disk that may lag collaborative edits.

## Extension toward partial rendering

- The core API returns **file input identities** and document-level closure, independent of the output representation (HTML, AST, or future fragments).
- A future `RenderUnitId` / `AffectedRenderUnits` projection can map `file → document → element or stage` without rewriting file-resolution, provenance, or preview sync events.
- This change does **not** introduce AST-element identities, incremental stage checkpointing, or fragment output; it simply removes dependency discovery from the UI and creates a scheduling seam.

## Performance rationale and open measurements

- **Avoids:** a second native dependency walk after each edit, extra include-shortcode parsing, and comparison with potentially stale disk files.
- **Adds:** construction of the existing `ProjectDependencyGraph` from the *already-produced* Pass-1 index, and a closure query. The core graph's sidebar co-membership expansion is potentially O(S²) for a sidebar with S pages; its build cost and the serialized snapshot size require realistic project-scale benchmarks. Do not claim a speedup until measured.
- **Planned:** warm/cold preview response latency and CPU profiles across 0/1/8/64 includes, 100/1000/3000 unrelated pages, large sidebars, and project with many listing globs; compare prior preview endpoint plus original/warm WASM renders; measure filtering false-positive and false-negative rates, not just median latency.

## Verification plan

- Rust graph test suite: full document and transitive include closure, shared dependencies, changes/removal, 3000 unrelated documents, identity cases.
- WASM adapter unit tests: known closure, navigation dependencies, symlink/out-of-project fail-open.
- SPA unit/component: decode validation, unrelated suppression, relevant update and added/deleted membership invalidation, stale async revision races.
- Fresh native q2 + WASM + SPA real-browser tests: nested include leaf change updates iframe, unrelated sibling edit does not bump render count, removing/re-adding include revises filtering, sidebar-linked document update triggers rerender.
- Broader regressions and benchmarks are future acceptance gates; record exact passing/failing/skipped counts from CI before a PR. **No benchmark claim or upstream PR is made here.**

## Known semantic limitations

- Graph edges cover core's recorded include, nav, body-link, sidebar, and listing channels. Arbitrary Lua filters, custom engines, external dependencies, JS assets, and non-source resources are not fully reflected in the input closure. Existing broad handling for non-source edit types remains. Investigate untracked dynamic source reads and fail-open policy before upstream release.
- Whole-document invalidation remains the only action the SPA can take today; an empty or incomplete graph must never suppress a source update.
- The old native `/api/preview/deps` route remains for compatibility but is no longer consulted by the SPA. It can be retired separately after maintainer review.
- Building a complete ProjectDependencyGraph for every live render can be costly with very large sidebars/listings; optimize only with comparative evidence.

## PR strategy

Keep this research branch and all CI evidence. Once tests and performance acceptance pass, prepare **one small, squashed upstream-focused PR** that reuses `quarto-core` and updates only the thin WASM and SPA integration. Preserve research commits outside the review branch. No upstream PR should be opened without explicit user approval.
