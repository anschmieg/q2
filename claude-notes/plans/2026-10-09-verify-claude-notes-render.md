---
title: 'cargo xtask verify does not run CI''s strict claude-notes render (bd-zn87vwi8)'
date: 2026-10-09
description: 'Adds a `q2 render --strict claude-notes` step to `cargo xtask verify`, reusing the debug q2 that verify already builds, so a note that CI rejects fails the local gate too.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-zn87vwi8
---

**Branch:** `main` @ `3b3e8aeaf` (main checkout; no worktree)
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The missing step is unambiguous, cheap (7 s with the debug binary), and has a direct precedent in the lint:css fix (bd-4bu7vwi5). Two questions are still open: where the step goes, and whether to reopen the "structural guard" decision that bd-4bu7vwi5 settled as "doc rule only".

## Issue context

Filed 2026-10-10 (UTC) by Carlos Scheidegger. Type `bug`, P2, open. Seen on PR #816 (bd-nbv80e33): full `cargo xtask verify` was green, then CI's "Render claude-notes (warnings are errors)" step failed on two qmd apostrophe errors in a plan note, Q-2-10 (`listings' sort-ui`) and Q-2-7 (an apostrophe after a code span). Plans and investigation notes go into claude-notes on nearly every branch, so most branches can hit this.

The strand proposes a verify step with a `--skip-claude-notes-render` flag, a `TOTAL_STEPS` bump, an explicit decision about `--skip-rust-build`, a clean tree afterwards, and a docs update. It also asks the broader question of how to stop verify and CI drifting apart.

## Dependency graph

- **related bd-7v3trpxn** (closed): "CI gate: q2 render --strict claude-notes on every push and PR". It added the CI step. Like bd-owmzraf6 before it (lint:css), it touched CI and not `verify.rs`. This is the second time the same drift has happened.
- **related bd-bomx9iyt** (open): the tree-sitter-doctemplate corpus runs in neither CI nor verify. That gap is in both gates, not drift between them, so it is out of scope here.
- Not linked but the same family: **bd-kshwnmpn** (q2-preview-spa e2e runs only behind `--e2e`), **bd-l7mcijfe** (Node-only CI suites missing from verify) and **bd-ya2nacaa** (CI legs that need Deno, wasm32 or the hub binary). The last two were filed by bd-4bu7vwi5 from its drift table (`claude-notes/plans/2026-09-09-verify-lint-css.md`).
- Precedent decision: on 2026-09-09, bd-4bu7vwi5 asked the same "structural guard" question (a lint that parses the workflows) and the answer was **"doc rule, no structural lint"**. That produced the AGENTS.md paragraph "Keep `verify` and CI in sync: when you add a gating step to a CI workflow, add its `cargo xtask verify` counterpart in the same commit." bd-7v3trpxn landed after that rule existed and still missed it, which is evidence that the doc rule alone is not enough.

## What the code looks like today

Checked at `3b3e8aeaf`:

- `crates/xtask/src/verify.rs`: `TOTAL_STEPS = 14`. Step 3 is `cargo build --workspace` under `RUSTFLAGS=-D warnings` (unless `--no-deny-warnings`), which produces the debug `q2`. Nothing renders claude-notes. `run_command` already threads the same `rustflags` through.
- The precedent guard test `css_lint_command_matches_ci_workflow` does `include_str!` on the workflow and asserts it contains `run: <local command>`. It is a one-step guard, not a general cross-check.
- `.github/workflows/test-suite.yml:239-248`: `cargo run --profile ci --bin q2 -- render --strict claude-notes`, Linux-only, `!cancelled()`. The `ci` profile is `inherits = "dev"` with reduced debuginfo, so it is effectively the same code as verify's debug build.
- `.gitignore` already ignores `claude-notes/_site/` and `claude-notes/.quarto/`. After a render, `git status` is clean (checked).

### Measurements (this machine, nextest running at the same time)

```
target/debug/q2 render --strict claude-notes
Rendered 1459 of 1459 files to .../claude-notes/_site
real 7.13   user 29.80   sys 4.49        exit 0
```

### Reproduction

`claude-notes/plans/verify-claude-notes-render-investigation/bad-note.md.txt` holds the two PR #816 patterns. The `.txt` suffix keeps it from being rendered. With a copy placed at `claude-notes/zz-tmp-bad.md`, the debug q2 printed the same Q-2-10 at 5:14 and Q-2-7 at 5:54 that CI reported, ended with `Rendered 1459 of 1460 files … — 2 errors`, and exited 1. I inspected the output and then deleted the copy. These are parse errors, so plain `render` also exits 1; `--strict` additionally catches the warning-only cases.

This plan hit the same trap while I was writing it. Its first draft used an indented code block, which `--strict` rejected. That draft would have passed verify and failed CI.

## Proposed phases (draft)

- Phase 0: Tests first.
  - A wiring/drift guard in the style of `css_lint_command_matches_ci_workflow`: the local render args (`render --strict claude-notes`) appear in a `run:` line of `test-suite.yml`.
  - A reachability guard like `pandoc_preflight_is_wired_into_run`.
  - Unit tests for the skip predicate (`runs_claude_notes_render(&config)`) covering `--skip-claude-notes-render` and `--skip-rust-build`.
- Phase 1: The step in `verify.rs`.
  - Run `cargo run --bin q2 -- render --strict claude-notes` through `run_command` with the same `rustflags` as step 3. Use `cargo run` rather than a hard-coded `target/debug/q2`, so `CARGO_TARGET_DIR` and Windows `.exe` are handled and the step is link-only after step 3.
  - Add the `--skip-claude-notes-render` flag in `main.rs` and `VerifyConfig`, and bump `TOTAL_STEPS`. Placement and numbering are design questions 1 and 2.
- Phase 2: Structural guard, if Q3 says yes.
- Phase 3: Docs.
  - The AGENTS.md "Full Project Verification" step list and skip options.
  - `claude-notes/instructions/writing-notes.md` (mention that verify runs it).
  - The CI step's comment in `test-suite.yml` (point at the verify counterpart).
  - The `verify.rs` module doc.
- Phase 4: End-to-end check. Run `cargo xtask verify --skip-hub-build` with the bad-note copy in place and confirm it fails at the new step with the CI report. Then run it without the copy and confirm it passes.

## Open design questions for the user

1. **Placement.** CI runs the render after nextest. I recommend putting it **right after step 3 (build)**, before the tree-sitter and nextest steps. Verify bails on the first failure, a 7 s check should fail fast instead of after a multi-minute nextest, and a note typo is the most likely failure on a docs-heavy branch. The alternative is to mirror CI and run it after step 5.
2. **Numbering.** Should it become a new top-level step (renumbering 4–14, `TOTAL_STEPS = 15`), or a sub-step of step 3 (`↳ claude-notes render`), the way lint:css sits inside step 1? A sub-step avoids renumbering every message and the AGENTS.md list.
3. **Structural guard: reopen the 2026-09-09 decision?** bd-4bu7vwi5 chose a doc rule with no lint. bd-7v3trpxn then drifted anyway. The options are:
   - (a) a per-step guard test only, as with lint:css (cheap, but it only protects steps someone already mirrored);
   - (b) an xtask test or lint that extracts every `run:` line from the gating workflows (test-suite.yml, ts-test-suite.yml, …) and requires each one either to match a verify command or to appear on an explicit CI-only allowlist with a reason and a strand. That catches the *next* new CI step at PR time.
   
   I recommend (b), as a separate follow-up strand, so this fix stays small. bd-l7mcijfe, bd-ya2nacaa and bd-kshwnmpn would seed its allowlist.
4. **`--skip-rust-build`.** Should it imply skipping the render? I recommend **yes**: without that flag, `cargo run` would quietly build q2, which contradicts the user's intent. In that case print `Skipping claude-notes render (--skip-rust-build)`. Should `--skip-rust-tests` leave the render on? I'd say yes, since the two are independent.
5. **Platform.** CI runs the render on Linux only. Verify runs everywhere, including Windows dev machines. Should it run on every platform? I recommend yes: parsing is platform-independent, so a Windows-only failure would be a real bug worth seeing.

## Risks / tradeoffs (draft)

- The step adds about 7 s, plus a link if the binary is stale. It is negligible next to nextest.
- Under `--no-deny-warnings`, `rustflags` is `None` and step 3 builds without `-D warnings`. `cargo run` must get the same flags or it will rebuild everything. CI hit this exact trap (PR #809, 12 min). Passing the same `rustflags` value through `run_command` covers it, but the step should be written so the two can't diverge, for example with a shared local.
- A guard test that matches a CI `run:` string is brittle if someone rewrites the CI invocation (`--profile ci`). Match the q2 args (`render --strict claude-notes`), not the whole cargo line.
