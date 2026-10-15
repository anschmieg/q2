# Core file dependency graph — research proposal (2026-10-15)

**Target:** `quarto-core::project::dependency_graph::ProjectDependencyGraph`, based on upstream `e308835bd062495d96424d1c8c2e56a5fe554818`.

This is deliberately **not** a preview fix. The original preview patch and its full 16-commit audit trail remain on `research/q2-m1-preview-deps-direct`; the proposed core changes are isolated on `research/q2-core-file-dependencies`. No default branch, upstream PR, or Quartier changes.

## Why

Q2 already distinguishes graph edges between *renderable document profiles* (sidebar, body links, navigation declarations, listings) and transitive included files recorded in `DocumentProfile.includes`. The former are indexed by `ProjectDependencyGraph`; the latter were used for cache validation, not for reverse file→document invalidation queries.

An included file can be an ordinary partial that **is not** a document in the renderable `ProjectIndex`. Putting it into the existing `edges` graph would change established document-to-document semantics and `forward_closure` / `always-render` behavior. We therefore add a separate file-input edge channel to the **same** core graph:

- `include_edges`: rendered document → recorded, **transitive** included file paths.
- `include_reverse_edges`: included file → documents whose AST contains that file.
- `include_dependents(changed_files)`: union of affected documents using only reverse lookups.
- `replace_include_dependencies(document, files)`: update one document after a successfully refreshed `DocumentProfile`, pruning stale reverse edges and keeping shared dependents.

## Performance intent (visible during upstream review)

- **No new parsing or IO.** Reuses `DocumentProfile.includes` produced by `IncludeExpansionStage` during the existing Pass 1. Also reuses the existing path identity (typically canonical absolute), without re-canonicalizing every edge.
- **Construction:** O(total included-file references) beyond the pre-existing profile/navigation graph work, with BTreeSet deduplication per document.
- **Query:** proportional to the reverse adjacency of the changed file(s), not the project size.
- **Update:** proportional to old/new included files of the one changed document, not a global `ProjectContext::discover` or graph rebuild.
- **Caveat:** map and set allocations cost memory; measure on realistic include-heavy projects before promising a numerical speedup. The 3,000-document unit case verifies index size/selectivity, not wall-time performance.

## Correctness contract and limitations

- This is an **index of a complete, known snapshot**, not automatic filesystem observation, caching, or a new parser. It relies on `DocumentProfile.includes` being up-to-date.
- The include path keys must match the recorded path identity. Symlink alias, missing profile, unknown/incomplete source or a race during discovery makes a negative result **unsafe**. Future callers **must fail open** (render more, never silently skip).
- Deleted files remain addressable by their *last recorded path*; no existence check is required in a reverse lookup. A successful re-profile replaces/removes old edges.
- The caller still handles a document's own source edit and all non-include input channels. This method does not claim to resolve bibliography, CSL, Sass, images, dynamic Lua effects, or cross-document profile invalidation.
- No change to existing navigation edges or render-set augmentation; nothing in `quarto-preview` or `q2-preview-spa` is touched.
- This is **document→file**, not element-level tracking. Element/stage-level IDs and side-effect barriers remain future research.

## Proposed tests

1. Transitive leaf and outer partials indexed even though neither is a separately renderable document.
2. Shared include dependents are returned without duplicates.
3. Re-profile a document: removed leaf no longer invalidates it; another document still dependent on that leaf is retained.
4. Empty include set prunes stale edges; newly added includes appear.
5. A deleted file is still findable from its prior path without disk reads.
6. An index with 3,000 unrelated QMD profiles holds only the one referenced include edge.

## Preview findings: a separate potential issue, not a new PR here

Upstream interactive preview uses a direct-include filter (`quarto-preview/src/deps.rs`) even though the core graph exists; it can miss nested changes, as shown by our [original targeted Chromium checks](https://github.com/anschmieg/q2/actions/runs/38767256307). Upstream's **static preview** uses conservative full invalidation for unknown input changes; calling *all preview fundamentally flawed* would overstate the evidence.

Possible later GitHub issues **after upstream discussion**, not opened here: (1) transitive interactive-preview include staleness, (2) stale disk-based dependency sets in editable/shared VFS sessions, (3) richer asset/bibliography/CSL invalidation. Keep them distinct from this core contribution.

## PR strategy

Submit **one core-first PR**, not two competing implementations. Its production diff should be small (the dependency graph plus tests), with a performance rationale and clear API contracts. First verify the branch against current upstream, then squash the reviewed code to a clean PR branch. Preserve the current full research, benchmarking and benchmark artifacts on their separate branches; do **not** force-push/delete them. An upstream PR is not open.

## Execution status

Implementation and test source are committed on this branch. **CI status must be checked before reporting any tests passed.** The endpoint benchmark is separate: [upstream vs original preview](https://github.com/anschmieg/q2/actions/runs/38788568356) and [three-way optimization](https://github.com/anschmieg/q2/actions/runs/38789100191). Those measurements address a possible *preview* optimization; do not conflate them with the core graph cost.
