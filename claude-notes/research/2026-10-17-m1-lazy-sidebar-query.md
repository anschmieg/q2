# M1.2 — Lazy sidebar membership for single-page live invalidation

## Problem

The existing `ProjectDependencyGraph::build` materializes a directed clique
for each sidebar, meaning a sidebar of N pages produces N × (N − 1) forward
edges **and** their reverse entries. M1.1 measured 512 sidebar members at
585 ms graph construction + 155 ms closure traversal (native Rust debug
runner). This graph is appropriate to preserve for current Mode-B consumers,
but wasteful for the active-page preview's single-document input closure.

## Implementation

- The core graph builder now has one shared private edge-collection path,
  `build_with_sidebar_mode`; normal `build` retains the original complete
  adjacency and reverse indices unchanged.
- A new core `render_inputs_for_active(index, metadata, diagnostics, active)`
  traverses a **sidebar hyperedge**: membership is indexed per path, and
  expanding any member visits its sidebar only once. Crossing overlapping
  sidebars, cycles, profile/body/nav/listing edges and transitive included files
  remains equivalent to the original closure.
- The WASM adapter uses this single-page API and retains the original
  diagnostics and fail-open contract. No parsing or filesystem reads were
  added; paths are still normalized and validated at the WASM boundary.
- Sidebar membership resolution now deduplicates with a `HashSet` while
  retaining its existing first-occurrence order, avoiding a separate quadratic
  membership scan.

**Scope:** The change removes quadratic sidebar-pair allocations only for the
single-page render-input query. The existing full graph's sidebar clique and
Mode-B semantics remain unchanged; replacing that API is a separate, potentially
breaking undertaking. The new query still scans all Pass-1 profiles and builds
non-sidebar profile/listing/include indices on each render, so no cross-edit
cache or M2 checkpoint reuse is introduced.

## Verification

- Core tests compare the lazy result against the established fully materialized
  graph for overlapping and automatic sidebars, transitive references,
  nonrenderable partials, cycles, and unrelated files.
- WASM adapter unit tests pin deterministic source-path snapshots for
  sidebar-linked pages.
- The CI benchmark directly compares the full graph build + query to the
  lazy single-page query, with five samples per scenario (32/128/256/512
  sidebar members plus flat project and listing cases). Output field
  `single_page_us_p50` is a native debug-build microsecond median.
- Real browser CI exercises project/single-file live include invalidation and
  unrelated-update suppression; no browser benchmark claims an end-to-end
  improvement from this isolated graph change without an A/B measurement.

**Acceptance:** full CI green, benchmark equality and measured scaling
reported. No default-branch edits, other repositories, agent delegation,
or upstream PR.

## Single-file recovery: explicit lifetime and trust limits

- Dependency retirement removes obsolete paths from the **live index** and
  therefore from future VFS membership/sync events. It does not delete the
  underlying samod/Automerge document; samod currently has no per-document
  garbage-collection API. The native `q2 preview` hub uses an ephemeral
  per-session data directory. Content already delivered to a collaborating
  peer is not revocable by removing its index entry. Durable document GC and
  peer revocation are outside M1.
- Native dependency enrollment verifies a canonical in-project path and
  rechecks containment at the read/sync site. Canonicalization followed by a
  file open is **not an atomic filesystem operation**. A malicious local actor
  simultaneously replacing symlinks/directories could exploit a TOCTOU window.
  The supported security boundary for M1 is a local project directory without
  adversarial concurrent filesystem writers; network peers cannot directly
  alter host symlinks unless the host separately grants that capability.
  Closing this OS-level race requires descriptor-relative contained opens
  and corresponding descriptor-based sync/write plumbing across platforms,
  not an unrelated patch to Quarto's global sync algorithm.
- A partial discovery failure must never retire still-existing dependencies.
  Missing dependencies that genuinely disappear may be retired so the browser
  receives a missing-include error and can recover after file re-creation.

The first two bullets are **accepted limitations**, not claims of complete
storage reclamation or race-free filesystem confinement.
