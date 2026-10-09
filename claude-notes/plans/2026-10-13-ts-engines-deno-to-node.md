# Plan: move TS engine extensions from Deno to Node

**Date:** 2026-10-13 (rewritten after plan review the same day)
**Status:** Outline; nothing started
**Branch:** `research/ts-engine-node-port`
**Research:** [`../research/2026-10-09-ts-engine-extensions-deno-to-node.md`](../research/2026-10-09-ts-engine-extensions-deno-to-node.md) (verified 2026-10-13)
**Review:** [`../research/2026-10-13-ts-engines-deno-to-node-plan-review.md`](../research/2026-10-13-ts-engines-deno-to-node-plan-review.md) (finding ids B1-B6, S1-S13)
**Review 2:** [`../research/2026-10-13-ts-engines-deno-to-node-plan-review-2.md`](../research/2026-10-13-ts-engines-deno-to-node-plan-review-2.md) (implementability; finding ids X1-X6, S1-S13 there are cited as R2-X*/R2-S*; this plan applies it)

## The problem

q2 runs TS engine extensions in a Deno subprocess (`engine-host-deno`). We want Node instead, so q2 has one JS runtime (`q2 mcp` already needs Node 24). The host, `@quarto/api` and the Rust protocol are already runtime-neutral; the work is one spawn layer, one host adapter, one transport, the bundler, the 12 fixture engines, marimo, julia, the extension build, and CI.

## Decisions

Made 2026-10-13 (before review):

1. **Dual runtime.** Extension bundles (marimo, julia) keep running under Quarto 1's Deno 2.x as well as q2's Node, so extensions use `node:` built-ins, not a `Deno` shim. Test against `~/src/quarto-cli` (Deno 2.7.14 per `configuration`) and keep a standing Q1 smoke leg in marimo and julia CI.
2. **Hard cut** to Node in q2. No flag, no two-runtime period in q2 (the dual-runtime *ports* land before the cut; see Phase order).
3. **Node floor 24**, reusing the discovery logic in `crates/quarto-mcp-launcher/src/node.rs` (`QUARTO_NODE`, PATH, fnm/nvm/volta).
4. **Rename** `@quarto/engine-host-deno` -> `@quarto/engine-host-node`, bundle `engine-host-node.js`.
5. **Bundler:** esbuild, pinned exactly.
6. **Out of scope:** `.ts` pre/post-render scripts (skill `q2-port-analysis:deno-to-node-porting`); publishing `@quarto/api` / `@quarto/types` (404 on jsr.io today; follow-up bead); `jupyter/text_execute.rs` "deno" kernel name; `quarto-system-runtime` deno_core removal.

Made 2026-10-13 (after review, Gordon agreed with each recommendation):

7. **Phase order:** dual-runtime ports first, one atomic flip, then cleanup (see Phase order).
8. **Node discovery:** lift `node.rs` into a tiny new crate `quarto-node-discovery` (re-exported by `quarto-mcp-launcher`); `quarto-core` does not depend on the launcher (it embeds the whole MCP bundle via `include_dir!`, pulls `fs2`/`sha2`/`dirs`, needs wasm32 gating, and its errors are MCP-worded). Error wording is parameterized by a purpose string; dependency is `cfg(not(wasm32))`. `QUARTO_NODE` now governs engines too; document it.
9. **Node floor guard:** drop the `import.meta.main` guard in the host `main.ts` (the bundle is always the entrypoint in control mode) instead of enforcing minor >= 2. Floor stays "major 24"; `.nvmrc`/`engines` unchanged.
10. **`build-ts-extension` bundler distribution:** embed an esbuild-driver bundle run with Node (same pattern as `quarto-mcp-launcher`), no `npx`. A spike (P6a) decides whether esbuild's native binary can ship; fallback is embedded `esbuild-wasm`. Unresolvable `jsr:` imports **fail with an actionable error** (not external, not translated). Pure `import type` of `@quarto/*` is erased by esbuild, but *value* imports (e.g. fixtures importing `@quarto/api/claims`) need the workspace alias tier (R2-X3). **Amended by review 2 (R2-X2):** the preferred design spawns the embedded esbuild binary directly from Rust, with no Node driver (see P6).
11. **marimo and julia are tested via forks.** marimo: Gordon's fork `gordonwoodhull/quarto-marimo` (`origin`; marimo-team is `upstream`) is the q2-facing integration line, as already done for julia. The shim's download URL and VERSION point at the fork's releases for testing; those changes live in a **separate fork-only commit** so the ported code can later be sent upstream without them. Upstream PRs are out of scope for this plan: it only prepares that code, and nothing here depends on upstream merging.
12. **Versions (Gordon, 2026-10-13):** the port is backward compatible but large, so bump the **minor** version of both: marimo `0.4.5` -> `0.5.0`, julia-engine `0.2.2` -> `0.3.0`. **marimo release:** bump VERSION and cut a Node-safe release (shim + bundle) on the fork as part of P4. q2's fixture keeps committing the real bundle in place of the shim.
13. **Extension tests use `node:test` + `node:assert/strict`** (same files run under `node --test` and `deno test --no-check -A` for the Q1 leg). Julia's Q1-native smoke tests stay as they are (the Q1 leg).
14. **External-repo build tooling:** marimo and julia each get `package.json` + lockfile pinning esbuild exactly, a repo-local build script, `setup-node`, eol handling for the committed bundle, and lose `deno.json`/`deno.lock`/import maps. P4/P5 therefore do not depend on q2's P6.
15. **Julia:** keep both detached-start paths (Unix `start_quartonotebookrunner_detached.jl` indirection, Windows PowerShell `Start-Process`), swapping `outputSync` for `spawnSync`; replace the socket read loop with a Buffer-accumulating line framer (fixing the empty-response `JSON.parse("")` bug); raw status output uses `process.stdout.write`, not `quarto.console.*`.
16. **Dead stdio branch** of the host `main.ts` is deleted (the Rust stdio transport was removed at the Phase-4 cutover) unless a test turns out to use it (check before deleting).
17. **Third-party engines:** the host rewrites `Deno is not defined` errors into an actionable message naming the extension and the migration doc; no `globalThis.Deno` shim. Migration note ships with the flip, not after.
18. **Test hygiene:** shared `node_available()`; Node-gated tests hard-fail under `QUARTO_CI=1` (replaces skip-as-pass); live baseline before P1 and per-phase delta accounting.
19. **Authorization (review 2):** the executor may (a) push NEW branches (never `main`, never force-push) to `gordonwoodhull/quarto-marimo` and `gordonwoodhull/quarto-julia-engine` named `q2/node-port`; rollback `git push origin --delete <branch>`; (b) push `research/ts-engine-node-port` to q2 origin for CI, with **no PR, no merge, no force-push**, polling `gh run list` / `gh run view --log-failed`; (c) cut the marimo fork release as the **last** P4 step (see P4). Nothing else outward-facing.
20. **Workspace runs (review 2):** this plan authorizes exactly two full workspace runs (baseline, P2 switch run; see Test strategy). Any other workspace run is a recommendation in the final report only.

## Verification corrections to the research

(From the first verification, 2026-10-13.)

- 12 files call `Deno.*`: the **10** hermetic fixture engines plus marimo's `src/marimo-engine.ts` and loader shim (which the P0/P4 re-sync replaces). `legacy-python` is a hand-written stub with none.
- `.github/workflows/test-suite.yml` also installs Deno, sets `QUARTO_CI`, and gates `deno_available_when_quarto_ci`; the research covered only `ts-test-suite.yml`.
- There are ~**11** `fn deno_available` copies (plus the `ts_process` test and two import aliases) across crates; they span `quarto-core`, `quarto-preview`, and `quarto` tests.
- No separate "Plan 9 one-shot deno process" exists; `ts_engine.rs` only wraps `is_available()`.
- `import.meta.main` exists on Node >= 24.2 (moot now: guard is dropped, decision 9).
- Julia subtree in q2 is byte-identical to the unmerged julia branch `q2-static-declarations`, not julia `main`.

## Corrections from the plan review

- **Fixtures:** only 10 hermetic fixtures exist (`HERMETIC_FIXTURES`); their `dist/` is gitignored and **rebuilt at test time** via `build_ts_extension` (`deno bundle`). Committed bundles exist only for legacy-python, marimo, julia.
- **marimo** commits no bundle upstream (its `_extensions/marimo/marimo-engine.js` is a Deno-only loader shim that downloads a release asset) and has no freshness check; only julia has a committed-bundle freshness gate.
- **marimo divergence:** `quarto-2-improvements` = `upstream/main` (78987bc) + 2 commits; q2's fixture comes from the separate `q2-bare-sql-interop` branch (based on 76d6f1d).
- Env semantics, stdout flush, and `import.meta.main` hazards are real (stdout truncation reproduced: 5 MB written then `process.exit` delivered 65536 bytes on Node).

## Test strategy (workspace runs)

A workspace `cargo nextest run --workspace` takes ~15-20 min and holds the cargo build lock, so it is scheduled, not sprinkled.

- **Baseline run (authorized):** one full workspace run on clean branch HEAD **before P0 starts** (before the fixture re-sync). At the same time save `cargo nextest list --workspace --message-format json` test *names* to the job tmp dir; diff names, not counts, because self-skipping tests count as pass. Run it in the background, tee to a log under the job tmp dir. P0's cargo-free marimo-repo work (rebase, `deno test`, pytest) proceeds during it; P0's q2 fixture re-sync and its cargo gate start only after the run exits. The P2 delta accounts for the P0 fixture change.
- **P2 switch run (authorized):** one full workspace run at the end of P2, same capture rules, paired with cargo-free P7 prep: migration note and `node:` table, doc edits (`Q-16-12.qmd`, `scripts.qmd`, Plans 1b/9/10), Windows checklist (for the final report), re-verifying marimo/julia bundles with `~/src/quarto-cli/package/dist/bin/tools/aarch64/deno` (2.7.14; run directly, never build inside `~/src/quarto-cli`), the final stray-`deno` grep. Report the delta against the baseline names list and account for it.
- **CI first, local when red:** push the P2 branch with `QUARTO_CI=1` so Node-gated tests hard-fail instead of skipping; run locally to attribute only if CI goes red.
- **Never overlap a run with** q2-side Rust edits or fixture changes. Keep P4/P5 (external repos) off the P2 run so attribution stays simple.
- **Narrow gates between:** the per-phase gates above; at P6 also `cargo nextest run -p quarto-core -p quarto -p quarto-preview`.
- **Skips are not passes (R2-S7):** for every gate naming `marimo_engine_e2e` or `julia_engine_e2e`, run with `--no-capture`, tee to a log, and require `grep -c '^SKIP'` = 0 on this machine (uv, Rscript, julia are installed). If a tool is missing, record per-test SKIP counts in the commit message and continue.
- **Hard-fail check (moved to the P2 gate, since no Node-gated tests exist before it):** `QUARTO_CI=1 PATH=<without node> cargo nextest run -p quarto-core ts_process::tests::node_available_when_quarto_ci` must FAIL.

## Phase order (every commit green)

Bundles that use `node:` built-ins run under the *current* Deno host, so ports land before the flip.

P0 (baseline run first) -> P1a -> P6 -> P3a -> P4 -> P5 -> **P2 flip (atomic)** -> P7

**Execution mode: straight line, no human stops (Gordon, 2026-10-13).** All decisions are made; run A to Z without waiting. Gordon tests the whole plan once it has landed. Report-style items below ("report ...") mean: write the finding into this plan or the commit message and continue. Stop and ask Gordon only if (a) a gate cannot be made green after attributing it from full-phase context, (b) a locked decision proves unworkable (e.g. the macOS extraction check fails even after the ad-hoc `codesign -s -` fallback, or a Q1 verification contradicts the dual-runtime premise); Windows/musl/Gatekeeper-download behaviour is not testable here and is never a stop, or (c) upstream already contains work that changes P0's scope materially.

Each phase ends with `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>` for crates touched, plus the gates named below. Workspace runs: only the two authorized ones (decision 20).

Baseline workspace run happens first, before P0 (see Test strategy).

### P0: reconcile marimo (fork `~/src/quarto-marimo`)

Runs first, still on Deno, so the port happens once on one codebase.

- [ ] **Setup:** work directly in the main checkout `~/src/quarto-marimo` (it was cleaned 2026-10-13: on `quarto-2-improvements`, no local changes; earlier scratch files are in `~/docs/marimo-scratch/`). First confirm `git status --short` is empty; if not, stop and attribute rather than stashing. Tag old tips `backup/pre-p0-quarto-2-improvements` and `backup/pre-p0-q2-bare-sql-interop`, then after the fetch create `q2/node-port` from `upstream/main` (`git switch -c q2/node-port upstream/main`) and cherry-pick in order: `quarto-2-improvements` commits, then `q2-bare-sql-interop`'s four. Rollback = `git switch quarto-2-improvements` and delete the branch (the backup tags keep the old tips).
- [ ] `git fetch upstream` (marimo-team; read-only, authorized); record the real gap between `upstream/main` and the two q2-relevant branches (last fetch was Jul 24). **Report, don't wait:** record what `upstream/main` already contains and whether any of this is already done upstream (escalate only if it materially changes scope).
- [ ] Rebase `quarto-2-improvements` (`claimsFile` -> false, static claims in `_extension.yml`) onto `upstream/main`; then rebase `q2-bare-sql-interop` (`LanguageClaim` interop, `cellOwnedByMarimo`, `buildCommand`, `extract.py` gate, new tests) on top. Expected conflict (verified with `git merge-tree`): `src/marimo-engine.ts` only; `tests/claims-language.test.ts` auto-merges; `_extension.yml` is untouched by the bare-sql branch.
- [ ] **Legacy fence syntax decision (Gordon, 2026-10-13):** q2 blesses only `` ```{python .marimo} `` (with or without the space, plus the dot-joined `{python.marimo}`); q2 does **not** support marimo's legacy `` ```python {.marimo} `` (it has a real parser). Background: the legacy form is marimo's Lua-filter-era syntax; Q1's `languagesWithClasses` regex (`pandoc-partition.ts:150`) requires braced fences so it never detects it, which is why (a) Gordon's `MARIMO_CELL_REGEX` (da94753, 2025-12-19; `\{?` makes the brace optional) feeds a custom `breakQuartoMd` split and (b) Péter Gyarmati's PR #62 (ab098d8, 2026-04-14) made `claimsFile` sniff for it. q2 refuses native-extension (`qmd`/`md`) claims outright (`source_conversion.rs`, D5) and never calls the dynamic `claimsFile` there.
- [ ] Therefore: keep the `claimsFile` sniff and the legacy-tolerant regex **for Q1 only** (dual runtime: existing Q1 users keep working), documented in the source as a Q1-only workaround. **Do not rewrite `f47802b`** (already in `quarto-2-improvements`; it deletes `containsMarimoFence`/`claimsFile`). After the rebase add a NEW commit restoring `containsMarimoFence` and the `claimsFile` sniff, marked Q1-only. Keep `claims-files: []` in `_extension.yml` for q2 and update its comment ("must agree with the runtime `claimsFile` (both say no)"). Determine whether Q1 reads yml `claims-files` (the Q1 repro below settles it) and record it. q2-facing tests assert only the blessed braced forms; keep the existing regex tests for Q1.
- [ ] Verify against real Q1 (`~/src/quarto-cli/package/dist/bin/quarto`; pass = a recorded yes/no per gap in the commit message): the two suspected Q1 gaps, unbraced fences and "first block of each language only" `firstClass`. Report which one #62 actually fixed.
- [ ] Verify (q2): the test-time `claims-files: []` patch in `marimo_engine_e2e.rs` is obsolete given D5; the diagnostic for legacy `python {.marimo}` fences is split off (strand `bd-iv9zxs5y`, separate session; not part of this port). Probe result (2026-10-13, q2 0.32.0, instrumented copy): `claimsFile` is never called in q2 for braced, legacy or plain files; the legacy fence yields a profile-pass parse error (exit 1), no spurious claim and no spawn, so no pass-1 incompatibility. Add a migration-guide note (`claude-notes/research/2026-07-03-marimo-migration-guide.md`).
- [ ] Resolve the static-vs-runtime claim contradiction: the YAML's `sql: {kind: interop}` is satisfied once the bare-sql branch's runtime `LanguageClaim` is merged; add a test that static and runtime `claims` agree; it explicitly exempts `claimsFile` with a comment citing the Q1-only rationale. Check how Q1 treats an object-valued claim (`engine.ts:186` uses it as a score) and that marimo is simply skipped for bare `sql` there.
- [ ] Ignore `~/src/quarto-marimo-main` (unrelated old line) unless something turns up. Record a conflict-resolution summary in the commit message, then continue to the fixture.
- [ ] Make the result the single fork branch q2 re-syncs from: push `q2/node-port` to `gordonwoodhull/quarto-marimo` (new branch only, decision 19).
- [ ] **After the baseline run has exited**, re-sync q2's `crates/quarto-core/tests/fixtures/extensions/marimo/` (src, lib, and a **freshly built committed bundle**, not the fork's `marimo-engine.js`, which is the Deno loader shim that downloads a release asset). The fork's `_extension.yml` declares all four static keys (`name`, `claims`, `file-extensions`, `claims-files: []`), which clears `Q-16-10` (verified on a scratch copy: no `Q-16-10`/`Q-16-11`, marimo output identical; control with `file-extensions` removed fires `Q-16-10` naming only that key). The test-time `claims-files: []` patch in `marimo_engine_e2e.rs` can then go; confirm by running the test without it. Check the fork yml's `filters:` entry (`marimo-deprecated.lua`) resolves in the fixture.
- [ ] Gate: `marimo_engine_e2e` (sc8, p4cb2, sc9, sc10, sc13, sc14, sc16, sc18, zero SKIPs) and `marimo_resolution` green on Deno; marimo's own `deno test` green, plus `tests/build-command.test.ts`, the new claims tests, and `pytest tests/python`.

### P1a: Node host core, added alongside the Deno files (`ts-packages/quarto-engine-host-*`)

No rename and no deletions yet; the Deno host keeps working.

- [ ] Baseline: the workspace baseline already ran before P0; additionally record the vitest count for the engine-host package.
- [ ] File names (defaults): `src/node-host.ts`, `src/node-main.ts`; bundle output `dist/engine-host-node.mjs` (`.mjs` so a project-local `"type":"commonjs"` `package.json` cannot break it), committed in P1a with a freshness-gate step. The embedded bundle switches at the flip. Always include the spike's `createRequire` banner (covers `blueimp-md5` CJS).
- [ ] `node-host.ts`: `PlatformHost` over `node:fs`, `process.env/cwd`, `child_process.spawn` (port `exec` timeout/mergeOutput/stderrFilter/respectStreams; on timeout kill then reject; handle `'error'`/`'close'`), recursive `readdirSync` for `walkSync`, a small async merge replacing `MuxAsyncIterator` (`jsr:@std/async`), `process.on("exit")` for `onExit` (synchronous), mktemp with suffix support (marimo needs `.html`).
- [ ] `isInteractive`: use `tty.isatty(0)`; never touch the `process.stdin` getter in control mode (it sets O_NONBLOCK and breaks `fs.readSync(0)` with EAGAIN).
- [ ] `control-transport.ts` over `node:net`: token read via `fs.readSync` on fd 0 so no byte past `\n` is consumed (H-READER); handle EAGAIN (short retry) and Windows' EOF throw; `socket.setNoDelay(true)`; writer awaits the write callback and drain, attaches an `'error'` handler that rejects pending writes, and `end()`s before exit; reader via the socket's async iterator (not `Readable.toWeb`).
- [ ] Node `main.ts` (control mode only): `process.argv`, no `import.meta.main` guard; delete the dead stdio branch (default: delete; if a test turns out to use it, port that test to the control transport); flush rule for stdout: `process.exitCode` + drain, never rely on `process.exit` after a large pipe write.
- [ ] Host diagnostic: catch module-load and call-time errors matching `/Deno is not defined/` and rewrite to an actionable message naming the extension path and the migration doc.
- [ ] Tests (vitest) for the Node host: fs/mktemp suffix, exec knobs, walk, onExit; `control-transport` against a `node:net` `createServer`; a test spawning the real bundle with piped stdin; a stdout test writing several MB then exiting and asserting complete receipt.
- [ ] esbuild config for the host bundle: `platform: 'node'`, `format: 'esm'`, `target: node24`, `external: ['node:*']` (no `jsr:*`); check byte-stability.
- [ ] One vitest case: import an extension bundle from a temp dir containing a typeless `package.json`; stderr stays empty.
- [ ] Bump the host devDependency `esbuild` from 0.28.0 to **0.28.2** (the spiked version), exact; regenerate the lockfile; rebuild the committed host bundle (so the ts-test-suite freshness gate and the embedded version agree).
- [ ] Update tsconfig/vitest excludes, bump `@types/node` to 24, typecheck the new files.
- [ ] Gate: vitest green; bundle builds and is byte-stable; Deno host and all existing Rust e2e still green.

### P6: extension build moves to esbuild (`extension/build.rs`, `q2 call build-ts-extension`)

Host is still Deno, which accepts esbuild's neutral ESM output.

- [x] **P6a spike (done 2026-10-13, esbuild/esbuild-wasm 0.28.2, Node 24.5.0): ship the native binary.** Embed the per-target esbuild binary (10-12 MB; 6 platforms total 65 MB, each `q2` build embeds only its own) plus a ~107 KB driver bundle; extract to a cache dir, `chmod +x`, set `ESBUILD_BINARY_PATH` before Node starts, run the driver. The driver bundle only works with `ESBUILD_BINARY_PATH` set and needs the banner `import{createRequire as __cr}from"node:module";const require=__cr(import.meta.url);`. `import type` of `@quarto/*` is erased with no alias. Output is byte-identical across runs and between native and wasm, **but only with a fixed relative entry path (or stdin)**: esbuild embeds the entry path in a comment, so the driver must pass a fixed relative entry. Native is 3-4x faster than wasm (~130-220 ms vs ~340-800 ms). Fallback only if extraction proves unworkable: `esbuild-wasm` (13.3 MiB `.wasm`; the Node API spawns literal `node` on an on-disk layout and cannot be bundled to one file; the in-process browser route needs stdin and a resolve plugin). Not yet tested: macOS quarantine/codesign of the extracted binary, Windows, musl. **P6 gate items (autonomous; this machine is macOS):** extract the embedded bytes to a fresh cache dir, `chmod +x`, `codesign -v`, run `<bin> --version` and one bundle build; on codesign failure fall back to ad-hoc `codesign -s -` after extraction; and byte-stability with the fixed relative entry. Windows and musl cannot be tested locally and are covered by the release-pipeline verify step. Only a macOS failure after the ad-hoc fallback escalates.
- [ ] **Binary delivery (R2-X2):** (1) `cargo xtask fetch-esbuild --target <triple>` runs `npm pack @esbuild/<plat>@<pinned>`, checks the lockfile integrity hash, writes gitignored `ts-packages/esbuild-embed/<triple>/`; called by `cargo xtask build-all` and the release-pipeline per-target leg. (2) The embed lives in the `quarto` bin crate or a new tiny `quarto-esbuild-embed` crate (build.rs with a placeholder fallback, `cfg(not(wasm32))`), **not** in quarto-core (a 10-12 MB `include_bytes!` there would link into every test binary and the wasm graph and undo bd-xvdop). A placeholder embed fails with "run cargo xtask fetch-esbuild". (3) `BuildOptions.esbuild_binary: Option<PathBuf>`; resolution: option, then `ESBUILD_BINARY_PATH`, then workspace `node_modules/@esbuild/<plat>/bin/esbuild` (tests use the last, via `npm ci`). (4) Add esbuild (MIT) to the third-party notices (precedent `release-pipeline.yml` ~L410-421); the release-pipeline per-target "Verify binary" step (~L606) also runs `q2 call build-ts-extension` on a tiny fixture (the only musl/Windows exercise). (5) Default invocation: spawn the embedded binary directly from Rust with `cwd=<ext dir>` and a fixed relative entry, `--bundle --format=esm --platform=node --target=node24 --external:node:* --conditions=source,import --alias:...`; no Node driver, no `ESBUILD_BINARY_PATH` dance, no Node at build time. P6 asserts byte-stability itself (two builds identical). Fall back to the Node driver bundle (spike layout, with banner) only if direct invocation cannot reproduce stable output.
- [ ] **CI for P6:** in `test-suite.yml` (Rust job), *after* "Free disk space" (`remove_tool_cache: true`, L167), add `actions/setup-node` node-version 24 and `npm ci` (needed for `node_modules/@esbuild/*` and the TS packages). Needed even with direct invocation if tests use the node_modules binary. P2's CI item keeps only "remove Deno".
- [ ] **Config (defaults):** four tiers kept (explicit `--config`, ext-local, workspace, shipped); config file is `esbuild.json` (`{"alias": {...}}`) in `resources/extension-build/`. Ext-local `deno.json` is **not** read; if present with no `esbuild.json`, print a warning naming it. Workspace tier aliases `@quarto/api`, `@quarto/api/claims` and `@quarto/types` to absolute `ts-packages/*/src/...` paths (as `deno.workspace.json` does today; no `node_modules` needed). Shipped tier has no value-import support for `@quarto/api`: fail with "not published; build inside the q2 workspace or pass --workspace". Unresolvable `jsr:`/`@std/*` imports fail naming the replacement `node:` module. Plumbing to rewrite: `resolve_build_config` (`build.rs:53-100`), `SHIPPED_DENO_JSON` (L138), the `q2-shipped-deno-` tempfile (L159), `run_deno_bundle` (L336-358), tests (L403-576), `crates/quarto/src/commands/build_ts_extension.rs:14`, and the two readers of `deno.workspace.json` (`quarto-preview/src/capture_driver.rs:543`, `quarto-core/tests/integration/engine_fixture_build.rs:50`) so P6 is green.
- [ ] Preserve what `build.rs` promises: resolution precedence (`build.rs:72-100`), lazy shipped-tier materialization and temp cleanup, `find_workspace_root`, `resolve_extension_dir`, no type-check (as `deno bundle` today), diagnostics/error text updated from "deno" wording. Keep the ~17 unit tests where behaviour is preserved; list deleted ones with reasons in the commit.
- [ ] Retire `resources/extension-build/deno.json` and `deno.workspace.json` (name both) once nothing uses them.
- [ ] Update `extension/mod.rs` docs and the native-gate comment.
- [ ] Update the host bundle config doc/comments that say `platform:'neutral'`/"Deno-ready" (`build_engine_host_bundle.rs`, `esbuild.config.mjs`) to `platform:'node'`.
- [ ] Gate: `engine_fixture_build`, `build_ts_extension_e2e`, `engine_diagnostics_cli`, `capture_driver`, all green (still need Deno at runtime for the host, not for the build).

### P3a: port the 12 fixtures to `node:` (still under the Deno host)

- [ ] Port `Deno.readTextFileSync`, `Deno.exit`, `Deno.stdout.writeSync` to `node:fs`/`node:process` in the **10** hermetic engines `crates/quarto-core/tests/fixtures/extensions/{alpha,beta,behave,content-claim,echo-engine,echo-legacy,fallback-univ,interop-r,mismatch,whenclass-marimo}/src/*.ts`; import `process`/`Buffer` explicitly. Marimo's two `Deno.` files are replaced by the P0/P4 re-sync: do not hand-port them. `legacy-python/dist/legacy-python.js` is a 901-byte hand-written stub (no source, no `Deno.`): leave it unchanged.
- [ ] Hermetic bundles keep being rebuilt at test time via `ensure_bundle` (not committed).
- [ ] Done check: `grep -rn 'Deno\.' crates/quarto-core/tests/fixtures/extensions --include='*.ts'` is empty except marimo.
- [ ] Gate: all engine e2e green under the Deno host.

### P4: marimo port (fork first, then re-sync fixture)

All on the fork's integration branch from P0.

- [ ] `executePython`: `quarto.system.execProcess` (preferred) or `node:child_process` (collect stdout/stderr as Buffers, decode UTF-8).
- [ ] `Deno.readTextFileSync`/`makeTempFileSync`/`writeTextFileSync` -> `node:fs` (+ `mkdtemp` and a fixed filename); `path` -> `node:path` (`fileURLToPath`); `delay` -> `node:timers/promises`; drop the remote `deno.land/std` import.
- [ ] **Loader shim** `_extensions/marimo/marimo-engine.js`: `Deno.stat`/`Deno.writeTextFile` -> `node:fs/promises`; `fetch` and dynamic `import` unchanged. Bump to `0.5.0` in both `marimo-engine.js` (`VERSION`) and `_extensions/marimo/_extension.yml` (`version`); the release tag is `v0.5.0`. The release itself is the **last** P4 step (see below). The release itself is the **last** P4 step (see below).
- [ ] **Fork-only commit** (kept separate from anything destined for upstream): shim download URL and VERSION pointing at the fork's releases, plus fork CI changes. Make the shim download URL overridable by an env var (default = fork release URL) so the Node end-to-end shim test (download + import) runs **before** any release, against a local `python3 -m http.server` serving the built bundle. The fork-only commit also replaces the `release-notification` step (needs `SLACK_WEBHOOK_URL_RELEASES`, absent in the fork).
- [ ] Tests: 4 files / ~559 lines (they use `Deno.test`, `jsr:@std/assert`, `Deno.makeTempDirSync`) -> `node:test` + `node:assert/strict`, run under both `node --test` and `deno test --no-check -A`; `@quarto/types` mock needs nothing (type-only imports); add a stub-python `executePython` unit test.
- [ ] Build: `package.json` + lockfile pinning esbuild exactly (same version as q2's); repo-local build script (from repo root, esbuild embeds relative path comments); replace `quarto call build-ts-extension` in the Makefile; `setup-node`, drop `setup-deno` from `ci.yml`/`publish.yml`; eol setting; delete `deno.json`/`deno.lock`. The existing `render` CI job (Quarto 1.9.x) stays as the Q1 leg.
- [ ] Verify the bundle runs under both Node (q2) and Q1 Deno 2.7.14 (`~/src/quarto-cli/package/dist/bin/tools/aarch64/deno`, run directly; `/opt/homebrew/bin/deno` is 2.9.0 and must not be used for this leg); grep the **built** bundle for hidden `Deno.*`.
- [ ] Re-sync q2 fixture; add a freshness gate for q2's committed marimo fixture bundle. Gate: `marimo_engine_e2e` green, zero SKIPs (still on the Deno host, since the bundle is dual-runtime).
- [ ] Fork CI gate: the committed fixture bundle equals the CI-built asset (identical pinned esbuild, fixed relative entry).
- [ ] **Last step:** push the `v*` tag on `gordonwoodhull/quarto-marimo` (fork only; `publish.yml` creates the release). Rollback: `gh release delete <tag> --cleanup-tag -R gordonwoodhull/quarto-marimo`.

### P5: julia port (fork `~/src/quarto-julia-engine` first, then re-subtree to `resources/extension-subtrees/julia-engine/`)

- [ ] **Base branch (default):** tip of `q2-static-declarations` (it already contains julia `main` and `upstream/main`; q2's subtree is byte-identical to it). Cherry-pick `b881e69` (redirect the detached server's stdio to devnull, needed by the detached-start item) and `4e6bc27`/`f7c9bfc` only if q2's subtree needs them; record the choice in the commit. Work on a new branch `q2/node-port` (decision 19). The julia repo has no unit tests: `tests/unit/` is new.
- [ ] `Deno.*` sites (count includes ones inlined from std: `fs/exists`, `jsr:@std/io`): `Deno.build.os` -> `process.platform === "win32"`; `Deno.env` -> `process.env`; `Deno.kill` -> `process.kill`; fs calls -> `node:fs`; `Deno.consoleSize` -> `process.stdout.columns ?? null`; `Deno.stdout.writeSync` -> `process.stdout.write`; `crypto.subtle`+`encodeBase64` -> `node:crypto` HMAC + `Buffer`; `Deno.RemoveOptions` -> `fs.RmOptions`.
- [ ] **Env rule:** every `Deno.Command` -> `child_process` swap passes `env: {...process.env, JULIA_LOAD_PATH: ...}` (Node replaces, Deno merges). On ENOENT check `.error.code`, not `.status`.
- [ ] **Socket framing** (`writeJuliaCommand`, ~L837-960): replace the 512-byte loop with a Buffer-accumulating line framer on `net.connect({host: "127.0.0.1", port})`. Tests against a fake TCP server (no Julia needed): message split across chunks including mid-UTF-8-character; multiple messages in one chunk; chunk ending exactly on `\n`; >512-byte and very large messages; close/error before reply and mid-message give a clear error (not SyntaxError); HMAC `{hmac,payload}\n` against a fixed vector; `isready` 10 s timeout destroys the socket; ECONNREFUSED/ECONNRESET surfaced; `error`/`juliaError` responses; progress-update loop; write backpressure.
- [ ] **Detached start:** keep both paths; `outputSync` -> `spawnSync(cmd, args, {encoding, env})`. `spawnSync` blocks while any grandchild holds the stdout pipe, so the Julia script must redirect server stdio. Windows PowerShell `Start-Process` path: untestable here; write a checklist item with pass criterion (call returns without hanging and the transport file appears) into the final report; not a stop; `QUARTO_JULIA` pointing at a `.cmd` may need `shell: true`.
- [ ] Bump `version` in `_extensions/julia-engine/_extension.yml` from `0.2.2` to `0.3.0` (also in the re-subtreed copy in q2). No julia tag or release is cut by this plan; the fork branch carries the bump and tagging is left to when the code is sent upstream.
- [ ] Julia's Q1 smoke tests (`tests/smoke/**`, via `run-tests.sh`) stay as the Q1 leg; new unit tests are `node:test`.
- [ ] Build/freshness: `package.json` + lockfile + pinned esbuild + `setup-node` + eol handling for the committed bundle (the freshness check also runs on the Windows matrix leg); CI step switches from `quarto call build-ts-extension` to the repo script.
- [ ] **Re-subtree from `q2/node-port` (verified feasible 2026-10-13):** the subtree's recorded `git-subtree-split` is `0a2b98fe`, the tip of `origin/q2-static-declarations`, and `q2/node-port` is based on that tip, so `git subtree pull --squash` sees a plain descendant. The branch must already be pushed to `origin` (decision 19). In q2: (1) change the `julia-engine` row in `crates/xtask/src/pull_extension_subtree.rs` (`subtrees()`, L54) to `remote_branch: "q2/node-port"` and update its doc comment (L37-41) and the assertion at ~L347-350 (`q2-static-declarations`); (2) update the same branch name in `resources/extension-subtrees/README.md` and `claude-notes/instructions/extension-subtrees.md`; (3) run `QUARTO_SUBTREE_ROOT="$(pwd)" cargo xtask pull-extension-subtree julia-engine` from the worktree (needed from a linked worktree; commit the row change first, since `git subtree` needs a clean tree); expect outcome "pulled". Verify the new `git-subtree-split` trailer equals the fork branch tip. If the branch later merges back into `q2-static-declarations` (or upstream), point the row back; that is a follow-up, not part of this plan.
- [ ] Gate: `julia_engine_e2e` j1-j7 and pc4a, zero SKIPs (still under the Deno host).

### P2: the atomic flip (Rust spawn to Node, rename, CI)

Pre-stage these, each landing green on its own before the flip (R2-S5):
- (a) `git mv ts-packages/quarto-engine-host-deno ts-packages/quarto-engine-host-node` plus package name, workspace deps, lockfile, tsconfig, xtask (`build_all.rs`, `build_engine_host_bundle.rs`, `main.rs`), `.gitignore`, `ci_test_wiring.rs`, doc comments, `ts_protocol.rs:1739`, `project/mod.rs:1400`; the embedded bundle file name stays until the flip.
- (b) `quarto-node-discovery` crate (lift from `quarto-mcp-launcher/src/node.rs`; launcher re-exports; `cfg(not(wasm32))` dependency from quarto-core), unused until the flip.
- (c) Shared helper: `#[doc(hidden)] pub fn node_available_for_tests() -> bool` in `quarto_core::engine::ts_process`; returns `is_available()`; panics when false under `QUARTO_CI=1`. Replace the ~11 `fn deno_available` copies (8 quarto-core integration files, `quarto-preview/src/capture_driver.rs:489`, two in `quarto/tests`) with it; its body still checks `deno` until the flip.
- (d) `DIALBACK_PREAMBLE` / dialback child rewritten as a runtime-parameterised helper, usable only at the flip.
- (e) Docs and migration note prepared.

**Atomic remainder (one commit/PR):** the three `Command::new("deno")` swaps, the helper body and embedded bundle swap, CI Deno removal, Deno file deletion. Everything below not covered by (a)-(e) lands together so no commit is red.

- [ ] New crate `quarto-node-discovery` (lift `find_node`, `Discovery`, `probe_version`, `parse_version`, `MIN_NODE_MAJOR` from `quarto-mcp-launcher/src/node.rs`; launcher re-exports; purpose-parameterized errors; `cfg(not(wasm32))` dependency from `quarto-core`).
- [ ] Rename `@quarto/engine-host-deno` -> `@quarto/engine-host-node` and the bundle to `engine-host-node.js`: package.json, workspace deps, `ts_process.rs` `include_str!` and bundle name `engine-host-deno-<sha8>.js` (L87, test L4291), `cargo xtask build-engine-host-bundle`, `.gitignore:54-61` and the package's `.gitignore`, `ci_test_wiring.rs` (TEST_MARKERS "deno test", test ~L324), xtask `build_all.rs`, `build_engine_host_bundle.rs`, `main.rs` (~410-457), `dev_setup.rs` (151, 339-362; `check_deno` -> Node check), `ts_protocol.rs:1739`, `project/mod.rs:1400`, plans/docs.
- [ ] Replace the three spawns in `ts_process.rs` (`ensure_started` ~L897, `is_available` ~L155, test helper ~L646) with `node <bundle> --control ...`; drop `--allow-all`, update the security-posture comment (Node has no permission model; same accepted v1 trust model) and error text; rewrite `DIALBACK_PREAMBLE` with `node:net`/`process`.
- [ ] `ts_engine.rs` / `registry.rs` `deno_is_available` wrappers -> Node; switch the shared `node_available_for_tests()` (pre-staged in (c); covering the copies, including `quarto-preview/src/capture_driver.rs` ~L481-580,638 and its `deno.workspace.json` path ~L543, and `quarto/tests/integration/build_ts_extension_e2e.rs`, `engine_diagnostics_cli.rs`) hard-fails under `QUARTO_CI=1`).
- [ ] `q2-preview-spa/e2e/engine-capture-splice*.spec.ts` and `e2e/README.md`: `denoAvailable`/`onPath('deno')` -> Node.
- [ ] CI: remove Deno (`DENO_VERSION`, setup/install steps) from `ts-test-suite.yml` (~L19-20,149-155,212-252) and `test-suite.yml` (~L19-21,143-155,217); Node 24 in the Rust job is already provisioned by P6 (`actions/setup-node` after "Free disk space"); do **not** set `QUARTO_NODE` in CI (`setup-node` puts Node 24 on PATH); `deno_available_when_quarto_ci` (~`ts_process.rs:4367`) -> `node_available_when_quarto_ci`.
- [ ] Delete `deno-host.ts`, the three `*.deno-test.ts` files, root `deno.jsonc`, and tracked `deno.lock`.
- [ ] Migration note for third-party engines ships here (see P7 content): `node:` equivalents table, dual-runtime recipe (env-merge caveat), Q1 `quarto-required` guidance.
- [ ] Sweep stale "Deno" wording in ~30 comments/test headers (e.g. `engine_registry_build.rs`, `marimo_engine_e2e.rs`).
- [ ] Gate (also run the hard-fail check from Test strategy): `ts_process.rs` unit tests, `echo_engine_e2e`, `behave_engine_e2e`, `synth_engines_e2e`, `synth_extension_subtree_e2e`, `capture_splice_seam`, `engine_registry_build`, `engine_fixture_build`, `build_ts_extension_e2e`, `engine_diagnostics_cli`, `marimo_engine_e2e`, `marimo_resolution`, `julia_engine_e2e`, preview `capture_driver`, the preview-spa engine-capture specs. Then run the second authorized workspace run (Test strategy); report the pass/skip and test-name delta against the pre-P0 baseline and account for it. Push `research/ts-engine-node-port` for CI (decision 19).

### P7: docs and cleanup

- [ ] Update `2026-04-16-ts-engine-extensions-subprocess.md` and Plans 1b/9/10 ("Deno must be on PATH"); user docs say q2 needs Node 24 at runtime (installer/release packaging has no Deno references and needs no change); `QUARTO_NODE` now governs engines.
- [ ] `docs/errors/extension/Q-16-12.qmd:53` (`deno run --allow-all`) and `docs/guides/projects/scripts.qmd:126`.
- [ ] Q1 `AGENTS.md` build-doc sync for julia/marimo: cannot be done in this repo; write it as a follow-up note in the final report.
- [ ] Final grep: `grep -rnw -i deno` minus an allowlist (`crates/quarto-core/src/project/render_scripts.rs:473`, `crates/quarto/Cargo.toml:89`, `quarto-system-runtime`, `jupyter/text_execute.rs`, `quarto-source-fetch/src/target.rs`). Pass = empty output.
- [ ] File one follow-up `braid` strand (out of scope of this plan): publish `@quarto/api` / `@quarto/types` (npm) so the installed-binary build path works.
- [ ] Reconcile this checklist with what landed before wrap-up (re-read each `- [ ]`/`- [x]` against the commits; correct; commit the plan).
- [ ] Final report lists residual human touchpoints: Windows checklist (spawn, `.cmd`, PowerShell detach, `readSync(0)`), Gatekeeper behaviour of a downloaded `q2`, Q1 `AGENTS.md` follow-up.

## Open risks

- Windows spawn, `.cmd` shims, detach/PowerShell path, `readSync` on fd 0: no Windows CI; manual checklist only.
- Third-party engines calling `Deno.*` will break (mitigated by the host diagnostic and migration note).
- esbuild native-binary distribution in the installed `q2` (P6a spike).
- Committed bundles change bytes under esbuild; Q1-side freshness checks must follow.
- Q1 verification used Deno 2.9.0 in review; re-verify on Q1's pinned 2.7.14.
- marimo-team `upstream/main` unchecked since Jul 24 (P0 starts with a fetch).
- Product-side dependency outside this plan: until the ported marimo/julia code is merged and released upstream, users installing from marimo-team get the Deno-only shim/bundle, which fails under q2's Node host.
