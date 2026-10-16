---
title: 'pandoc.read from Lua leaks "[TOP-LEVEL MISSING NODE]" lines for pipe-table delimiter dashes (bd-okfxemnj)'
date: 2026-10-15
description: 'The Lua pandoc.read binding wires the qmd reader''s verbose trace buffer to stderr, so marker tokens that hit the tree-sitter dispatch catch-all print one line each; route the buffer to a sink and give the tokens explicit arms.'
status: in-progress  # Go-ahead given 2026-10-16; see Progress checklist
braid:
  strand: bd-okfxemnj
github:
  issue: 847
---

**Branch:** `braid/bd-okfxemnj-pandoc-read-delimiter-dash-noise` in the main checkout (topic branch, no worktree), based on `main` @ `e308835bd`
**Go-ahead given 2026-10-16.** Progress is tracked in the checklist below.

## Progress checklist

- [x] Phase 0a — reader-level test `crates/pampa/tests/integration/test_trace_buffer_silence.rs` (3 tests; all fail on the unfixed tree with the predicted `-`, `pipe_table_align_*`, `![` trace lines)
- [x] Phase 0b — e2e test `crates/quarto/tests/integration/pandoc_read_stderr_e2e.rs` (fails on the unfixed tree with the issue's six lines)
- [x] Phase 1 — `lua/readwrite.rs`: trace buffer → `VerboseOutput::Sink` (e2e test green with this alone)
- [x] Phase 2 — `treesitter.rs`: explicit arms for `-`, `pipe_table_align_left`, `pipe_table_align_right`, `![` (reader-level tests green)
- [x] Both tests green; `pampa -v` on `probe.qmd` prints no catch-all lines. Also checked on the unfixed tree that hyphen-heavy text (`foo-bar`, `a - b`, `[a-b](c-d "t-u")`, `<http://a-b.c/d-e>`, `` `x-y` ``, `$a-b$`) emits no bare `-` trace line, so the new `-` arm only ever sees delimiter-row dashes (`hyphens.qmd` in the investigation dir).
- [x] `cargo xtask verify --skip-hub-build` green on the branch (2026-10-16)
- [x] Phase 3 — PR opened (`Fixes #847`); see the strand comment for the URL
- [ ] Strand closed after merge

## Triage verdict

**Ready to design.** The symptom reproduces at HEAD exactly as GH #847 describes, the two causes the issue names are both confirmed, and the fix is two small, independent, behavior-preserving changes plus tests. The open questions are about scope (how much of the surrounding hygiene to pull in), not about the fix itself.

## Issue context

[GH #847](https://github.com/quarto-dev/q2/issues/847) (cscheid, 2026-10-15, label `bug`). Calling `pandoc.read(text, "markdown")` from an `{lmd}` cell (or a Lua filter) on text containing a pipe table prints

```
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
```

once per dash in the delimiter row. The table renders correctly; the lines are unconditional, unformatted (no code, no location), unaffected by `--strict` / `-v`, and on a page that builds a large table from data they drown real diagnostics. The literal pipe table in the same document prints nothing. Found while porting a docs site whose config tables are built by an lmd function via `pandoc.read` (see #846 for why `pandoc.read` is the route in use).

Braid strand `bd-okfxemnj` was created for this investigation (type `bug`, priority 2, label `gh-847`).

## Dependency graph

The strand was created on 2026-10-15 from the GitHub issue with no incoming edges. The only external pressure is the issue itself and its sibling #846 (the reason `pandoc.read` is on the hot path for the docs port). No other strand is waiting on this.

Two follow-ups were filed `discovered-from:bd-okfxemnj` on 2026-10-16 (see Design decisions):

- `bd-fwswbxnl` (bug, p3) — `pandoc.read` from Lua drops the qmd reader's diagnostics.
- `bd-yi613i1r` (task, p4) — make the tree-sitter dispatch catch-all visible to tests.

## What the code looks like today

### Reproduction at HEAD

Reproduced with a debug build. The issue's `repro.qmd` prints six `[TOP-LEVEL MISSING NODE]` lines (two columns, three dashes each) and renders two correct tables. Artifacts and exact outputs are in `claude-notes/plans/pandoc-read-delimiter-dash-noise-investigation/README.md`.

### Cause 1: the catch-all fires for marker tokens the parent consumes

`crates/pampa/src/pandoc/treesitter.rs` converts the tree-sitter concrete tree bottom-up: `treesitter_to_pandoc` (line ~1853) walks every node, including anonymous tokens, and calls `native_visitor` on each. `native_visitor` dispatches on `node.kind()`; the last arm (line ~1819) is a catch-all that writes

```rust
writeln!(buf, "[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: {}", node.kind())
```

and returns `IntermediateUnknown(range)`.

The grammar rule for a delimiter cell (`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:363`) is

```js
pipe_table_delimiter_cell: $ => seq(
    optional(alias(':', $.pipe_table_align_left)),
    repeat1('-'),
    optional(alias(':', $.pipe_table_align_right)),
),
```

so every dash is an anonymous `-` child and every colon a named alias. None of the three kinds has an arm in `native_visitor`, so each one hits the catch-all. The parent processor, `process_pipe_table_delimiter_cell` in `treesitter_utils/pipe_table.rs:23`, then matches children **by kind name** (`"-"`, `"pipe_table_align_left"`, `"pipe_table_align_right"`) and ignores the intermediate value, which is why the output is correct regardless.

The codebase already has an established pattern for exactly this situation: marker tokens get an explicit arm that returns `IntermediateUnknown(node_location(node))` with a comment saying the parent consumes them. Examples at `treesitter.rs` ~778–792: `atx_h1_marker`…`atx_h6_marker`, `task_list_marker_checked`/`_unchecked`, `"$" | "$$"`; and the `*_delimiter` arms around lines 999–1094. The delimiter-row tokens are simply missing from that list.

A probe document (`probe.qmd` in the investigation dir) run through `pampa -v` shows the complete set of kinds that reach the catch-all on ordinary input: `-`, `pipe_table_align_left`, `pipe_table_align_right`, and `![` (the anonymous opener of `pandoc_image`, likewise consumed by name in `span_link_helpers.rs`). Lists, task lists, quotes, footnotes, divs, shortcodes, math, and the `|` separators do not reach it.

### Cause 2: the Lua binding is the only production caller that points the buffer at stderr

`readers::qmd::read` takes an `output_stream: &mut T: Write`. It is a verbose/debug trace: the `pampa` CLI routes it to stderr only under `-v` (`crates/pampa/src/main.rs:187`, via `utils::output::VerboseOutput::{Stderr, Sink}`), and every pipeline caller passes a sink or a throwaway `Vec` — `parse_document.rs`, `lmd/desugar.rs`, `include_expansion.rs`, `engine_execution.rs`, `listing_render.rs`, `quarto-preview/deps.rs`, `quarto-lsp-core/diagnostics.rs`, and the two other Lua-adjacent readers `lua/config_value.rs:689` (`VerboseOutput::Sink`) and `pandoc/meta.rs:153`.

The one exception is `pandoc_read` in `crates/pampa/src/lua/readwrite.rs:262`:

```rust
let mut stderr = std::io::stderr();
match crate::readers::qmd::read(&content, false, "<pandoc.read>", &mut stderr, true, None) {
```

That line has been there since the file was created (it predates the crate rename `d86b8ecf4`). It is the only reason the trace reaches the terminal, and it explains every property the issue lists: no diagnostic code, no location, not gated by `-v`, not affected by `--strict`.

An experiment replacing that one `stderr()` with `sink()` makes the repro render silently with both tables intact (change reverted).

### A neighbouring gap worth naming (not the bug)

The same `pandoc_read` arm discards the reader's real diagnostics: `Ok((pandoc, _context, _warnings))`. A `pandoc.read` of markdown that triggers, say, Q-2-54 (definition list) produces no warning at all, whereas the same text in the document body would. The Lua runtimes already have a channel for this — `quarto._diagnostics`, drained by `extract_lua_diagnostics` in `lua/filter.rs:308`, `lua/shortcode.rs:297`, and `lua/lmd.rs:185` — but the reader's diagnostics carry `SourceInfo` into a fresh `ASTContext` whose file `<pandoc.read>` is not registered in the document's `SourceContext`, so forwarding them naively would mis-render locations. This is a separate, slightly larger piece of work; see design question 2.

## Proposed phases (draft)

- **Phase 0 — Tests (written first, failing).**
  - Reader-level: a test in `crates/pampa/tests/integration/` that reads a pipe table exercising all four alignments plus an inline image into a `Vec<u8>` buffer and asserts the buffer is empty. Fails today with the four kinds listed above. Guards Phase 2 and documents that the trace buffer is expected to be silent on well-formed input.
  - End-to-end: a `q2 render` test in `crates/quarto/tests/integration/` (same `CARGO_BIN_EXE_q2` pattern as `brand_font_weight.rs`) that renders the issue's `repro.qmd` and asserts stderr contains no `[TOP-LEVEL MISSING NODE]`. Fails today; guards Phase 1 independently of Phase 2.
- **Phase 1 — Route the Lua reader's trace buffer to a sink.** In `lua/readwrite.rs`, replace `std::io::stderr()` with `VerboseOutput::Sink(std::io::sink())`, matching `lua/config_value.rs`. One line plus a comment explaining that the buffer is a `pampa -v` trace, not a diagnostic channel. This alone closes #847.
- **Phase 2 — Give the marker tokens explicit arms.** In `native_visitor`, add `"-" | "pipe_table_align_left" | "pipe_table_align_right"` (consumed by `process_pipe_table_delimiter_cell`) and `"!["` (consumed by `process_pandoc_image`) returning `IntermediateUnknown(node_location(node))`, with comments in the house style. Behavior-preserving: the catch-all already returned the same value; only the trace line goes away. After this, `pampa -v` on the probe prints no catch-all lines, so the catch-all regains its meaning as "a genuinely unhandled kind".
- **Phase 3 — Wrap up.** PR with `Fixes #847`; close the strand. No user-facing docs change: nothing documented changes, noise is removed.

Phases 1 and 2 are independent; either can land alone.

## Design decisions (2026-10-16)

Answered by the user; the questions as originally posed are kept for the record.

1. **Scope: land both Phase 1 (sink) and Phase 2 (explicit arms).** Phase 1 fixes the user-visible bug; Phase 2 is four token kinds in the existing pattern and keeps the catch-all honest for `pampa -v` users.
2. **Forwarding reader diagnostics from `pandoc.read` is out of scope here.** Filed as `bd-fwswbxnl` (`discovered-from:bd-okfxemnj`). It needs its own decision about how `<pandoc.read>` text gets registered in the document's `SourceContext`, or whether locations are stripped, and would hold up the one-line fix.
3. **Making the catch-all louder in tests is out of scope here.** Filed as `bd-yi613i1r` (`discovered-from:bd-okfxemnj`, low priority). Options recorded there: a `tracing::debug!` alongside the trace write, or growing the Phase 0 reader-level test into a corpus sweep across `crates/pampa/tests/` fixtures. Replacing the reader's `buf: &mut T` plumbing (200+ call sites) stays out of scope.
4. **Include the end-to-end `q2 render` stderr test.** It is the only test that observes the Lua binding's buffer choice directly; the unit-level `readwrite.rs` tests cannot see stderr in-process.

Implementation starts only on the user's explicit go-ahead.

## Risks / tradeoffs (draft)

- **Low risk.** Both code changes are behavior-preserving on the AST: Phase 1 changes where a debug string goes, Phase 2 changes which arm returns an already-identical value.
- **`"-"` as a global arm.** Adding a top-level arm for the anonymous `-` token applies to any parent that has an anonymous `-` child, not just delimiter cells. Today every such parent already receives `IntermediateUnknown` from the catch-all, so the arm cannot change any parent's input; it only silences the trace. If a future grammar change introduces a `-` child somewhere unexpected, the arm's comment should make that discoverable. Scoping the arm by checking `node.parent()` is possible but not in the house style and not worth it.
- **The probe's "complete set" is for ordinary input.** Other constructs may reach the catch-all on error-recovery trees. Phase 2 deliberately handles only the four kinds observed on well-formed input; the catch-all stays for everything else.
- **Pre-flight `cargo xtask verify --skip-hub-build` at `main` @ `e308835bd`:** green (all 14 steps passed, 2026-10-15).
