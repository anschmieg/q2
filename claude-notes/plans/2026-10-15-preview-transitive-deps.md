---
title: 'Project preview transitive include dependencies'
date: 2026-10-15
description: 'Use renderer include records to keep project preview current while excluding unrelated edits.'
status: in-progress
---

## Scope and baseline

Work is confined to `anschmieg/q2`, branch `research/q2-m1-preview-deps`.
Baseline: `d110cf7e47d37f9b9771ba5cc7d92a1855934d4a`.
Upstream checked on 2026-10-15: `e7d4eab9f16de81c2dcd2b80b1253e5b9fd98c15`.
The upstream diff does not change `deps.rs`, `config.rs`, `PreviewApp.tsx`,
or the include expansion stage. Retain the baseline to make comparison reproducible.
No default branch writes, PR, upstream contact, deployment, or Quartier edits.
Task coordination and evidence belong to Paperclip task ADR-34.

## Tests first

- [ ] Inspect native endpoint, renderer, VFS synchronization, and SPA test harness.
- [ ] Record a failing project regression: index includes outer, outer includes leaf;
  leaf edits rerender index and unrelated sibling edits do not.
- [ ] Cover dependency additions and removals, missing and recreated paths, cycles,
  project-relative normalization, symlinks and containment, stale/error responses,
  and rapid updates with newest results winning.
- [ ] Implement closure with existing parse and include expansion stages and
  `DocumentAst::recorded_includes`. Do not add an include parser.
- [ ] Verify focused tests, compare clean render bytes against the baseline,
  and run feasible native, WASM Lua, and browser WASM regressions separately.
- [ ] Record commands, commit IDs, results, resource limits, and rollback steps;
  publish code and evidence only on the authorized research branch.

## Design questions to settle from code

Dependency discovery must read the same document state as preview rendering.
An empty known set differs from unknown or incomplete discovery. Errors must
allow conservative rerenders; disk data must not certify an unsaved VFS snapshot.
Refresh on relevant edits and file membership changes, and reject stale requests.
Use renderer path semantics, preserving project containment and VFS key identity.
Do not change renderer output to solve an invalidation problem.

## Verification and resource policy

Start with the endpoint and SPA harness tests, preserving RED output before edits.
Use the repository nightly Rust pin and Node 24. Run workspace build, nextest,
and xtask verification where resources permit, plus the existing WASM suites.
Report blocked commands as blocked, never as passes. The initial host has 16 GiB
free disk and no rustup on PATH; inspect tool availability before large builds.
Plan-only first commit is explicitly required by the assignment; full code tests
are not a prerequisite for that documentation commit. No snapshots changed.
