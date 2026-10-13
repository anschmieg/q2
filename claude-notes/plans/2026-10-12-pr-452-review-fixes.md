# PR #452 review fixes: no-WASM build leg, render timeout, small correctness items

## Overview

Local fix work on top of PR #452 (`feature/hub-mcp-get-errors-v2`,
branch `pr-452-fixes`), following the review at
<https://github.com/quarto-dev/q2/pull/452>. main was merged in
(2026-10-12, zero conflicts) because the headline finding only exists
against main's CI topology (the `workspace-ts-suites` job from GH #250).

**Do not push until the user approves the review comments.**

## Checklist

- [x] Merge `main` into the PR branch (stands in for the rebase; zero conflicts)
- [x] No-WASM build leg: stub `wasm-host.mjs` when `crates/wasm-quarto-hub-client/pkg/` is absent
  - `buildWasmHost(outDir, { allowStub })`: package `build` allows the stub; `bundle.mjs` (release path) refuses it
  - Gate the real-WASM test files (`local-render.test.ts`, `get-errors-live.test.ts`, `bundle.test.ts`) on host availability
  - Verify: clean worktree without `pkg/` builds and tests green
- [x] Render timeout: write-check degrades after timeout; `get_errors` errors cleanly
- [x] `loadHost()` retries after a failed init; `handleGetErrors` catches render failures into a clean tool error
- [x] ANSI strip: `/\x1b\[[0-9;]*m/g`, shared helper, also applied to sibling `failure.error`
- [x] Read-only mode: do not register the `fix_errors` prompt
- [x] sass comments: `wasm-host-entry.mjs` + v2 plan doc contradict the build script (sass is bundled)
- [x] `get_errors` description: note large-project latency vs client timeout
- [ ] Braid strand for the embedded-WASM dedupe follow-up — DEFERRED, braid publishes on create; do after approval
- [x] Verify: `npm run build` + `npm test` (quarto-hub-mcp), no-WASM worktree run, `cargo xtask lint`, dist smoke check

## Out of scope (author's call after the review lands)

- Moving the `scripts/local-prod-server.mjs` WebSocket fix to its own PR
- Lowering `MAX_CHECKED_DOCUMENTS` or incremental VFS fill (perf)

## Merge-surface discoveries (the reason the rebase mattered)

Developed against the August base, the PR's host worked; against
current main it did not. Three breakages, all fixed here:

1. dart-sass 1.100.0's `node` export condition resolves `import "sass"`
   to `sass.node.js`, whose CJS `require("util")` dies inside the
   esbuild ESM bundle. Fixed by aliasing `sass` to its pure-JS
   `sass.default.js` entry (browser parity).
2. dart-sass's dart2js runtime detects Node and calls `require("url")`;
   esbuild's `__require` shim throws in plain Node (vitest's runner
   masks it). Fixed with a `createRequire` banner on the host bundle.
3. The sass bridge's importer resolves through `setVfsCallbacks`,
   which JS hosts must wire (preview-runtime does; the August host did
   not). Without it every theme compile fails "Can't find stylesheet
   to import" (`vendor/rfs`). `wasm-host-entry.mjs` now wires it.

Verification record: full package suite 291 passed / 3 platform
skips; bundle.test.ts green (release path, real WASM);
`cargo xtask lint` green; plain-Node double-render repro clean.

## Additional points (2026-10-13)

- Incremental VFS fill: implemented (payload-identity diff; one full
  fill per project state instead of one per render).
- local-prod-server.mjs drive-by: DROPPED from this branch (user
  decision 2026-10-13 — no behavior regression: the branch never
  touches the local-dev proxy, and main behaves this way today). The
  fix remains in the PR's original commits on origin; the user will
  file it as a separate PR (2026-10-13).
- Docs check: no user-facing source page for the MCP tools exists on
  main (only stale _site output) — nothing small to fix; a tools guide
  is a follow-up candidate.
- Braid strand for the WASM dedupe follow-up: dropped (user decision
  2026-10-13) — the follow-up is already public in the PR description
  and in the v2 plan's backlog, so a strand would be bookkeeping only.
