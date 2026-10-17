---
title: 'pandoc.utils.stringify returns an empty string for a pandoc.Nodes value (bd-lt7rgi90)'
date: 2026-10-16
description: 'stringify_value in pampa''s pandoc.utils dispatches on LuaInline and LuaBlock only, so a pandoc.Nodes value silently stringifies to ""; add a LuaNodes arm and decide what the other pandoc.utils dispatchers and the unknown-userdata fallthrough should do.'
status: implemented  # design settled 2026-10-16, implemented on the branch the same day
braid:
  strand: bd-lt7rgi90
---

**Branch:** `braid/bd-lt7rgi90-stringify-nodes` in the main checkout (no worktree, per the user's request), based on `main` @ `8e0345009`, then merged with `origin/main` once PR #859 landed.

## Decisions (Carlos, 2026-10-16)

1. **Sequencing**: PR #859 merged while the investigation was open, so
   the branch merged `origin/main` and dropped the `pandoc.Span(...)`
   workaround in the #859 test itself.
2. **Scope**: the sibling fixes are in this strand: `blocks_to_inlines`
   accepts a `Nodes`, and `LuaNodes` gets a structural `__eq`.
3. **Fallthrough**: option (b). Unknown userdata is a `Q-11-3` error;
   `nil` stays `""`. Registered as D-stringify-nil in
   `crates/pampa/tests/lua-conformance/divergences.md`. Citation, meta
   proxies, Attr, ListAttributes, config null and config specials got
   real arms rather than errors.
4. **Blocks-shaped Nodes**: delegate to `stringify_blocks` and inherit
   whatever it does, to keep one code path (bd-0jtai5dh).

## What was done

- `crates/pampa/src/lua/utils.rs`: `stringify_value` takes the `Lua`
  handle and dispatches on Nodes (by shape), Citation (prefix + suffix),
  meta proxy (`stringify_config_value`: prose through the inline/block
  walk, arrays and maps concatenated, maps in key order, scalars through
  `push_config_value` then back into `stringify_value`), config specials
  (their text), and `Attr` / `ListAttributes` / `quarto.config.null()`
  (`""`); any other userdata is `Q-11-3: ... expected, got <type>`. The
  userdata-naming branch of `pandoc.utils.type` moved into
  `userdata_type_name` so the error names types the same way.
  `extract_blocks` (behind `blocks_to_inlines`) accepts a `Nodes` alone
  or as a list element, via `to_blocks`.
- `crates/pampa/src/lua/nodes.rs`: `LuaNodes::structurally_eq` and the
  `__eq` metamethod; two empty Nodes are equal regardless of shape.
- `crates/pampa/src/lua/json.rs`: the two `stringify_value` callers pass
  `lua`; their Inline/Block guard is unchanged, so `pandoc.json.encode`
  of a Nodes is still null (deliberate non-change).
- Tests: six new in utils.rs (Nodes, Citation, Attr/ListAttributes/null/
  specials, rejection, `blocks_to_inlines` with Nodes), one in nodes.rs
  (`equality_is_structural_over_shape_and_contents`), one in metadata.rs
  (`meta_proxy_stringifies_like_a_metamap`); the Span wrapper removed from
  the D-numtext test in constructors.rs.
- Docs: none needed. The lmd and components guides use stringify on meta
  values and Inlines lists, which already worked; no page describes
  stringify on a Nodes.
- Verification: `cargo nextest run -p pampa` 5122 passed (conformance
  included), clippy `-D warnings` clean, fmt clean; repro output after
  the fix captured in the investigation README.

## Pre-flight

`cargo xtask verify --skip-hub-build` at `main` @ `8e0345009`: 15780 of
15781 tests passed. The one failure,
`quarto-preview::integration editor_ui::editor_ui_serves_pdfjs_from_the_viewer_bundle`
(404 for `/pdfjs/web/viewer.html`), is environmental: the compile-time
embed directory `hub-client/dist-preview-embed/` on this machine dates
from 2026-09-17 and predates the pdfjs tree, and `--skip-hub-build` does
not refresh it. Unrelated to pampa's Lua layer; a hub build would clear
it.

## Triage verdict

**Ready to design.** The bug is a one-arm omission in a single dispatch
function, reproducible at HEAD, with a clear fix. What still needs a
decision is the blast radius: whether the sibling `pandoc.utils`
functions get the same arm, whether the silent `""` fallthrough for
unknown userdata survives, and how the fix sequences against the open
PR #859 that the strand was discovered from.

## Issue context

Filed 2026-10-16 by claude (bug, P3, labels `lua`, `pampa`) while writing
the D-numtext constructor tests on PR #859. `pandoc.utils.stringify` on a
`pandoc.Nodes` returns `""`. The test on that PR works around it by
wrapping the Nodes in a `pandoc.Span` (see
`numbers_and_booleans_read_as_text_in_inline_positions_only` in
`crates/pampa/src/lua/constructors.rs` on branch
`feature/bd-q4gnd5os-numtext-inline-coercion`, around line 4779:
`s(pandoc.Span(pandoc.Nodes(42)))`).

The strand's description names the cause correctly: `stringify_value`
(`crates/pampa/src/lua/utils.rs:786`) borrows the userdata as
`LuaInline`, then `LuaBlock`, then returns `Ok(String::new())`.

Two premises in the description need adjusting:

1. **The exact repro depends on PR #859.** On `main`, `pandoc.Nodes(42)`
   errors with "got number"; only PR #859 makes numbers acceptable to the
   `Nodes` constructor. The stringify gap itself is independent of #859 and
   reproduces on `main` with `pandoc.Nodes('42')` or
   `pandoc.Nodes({'a', '2'})`.
2. **Pandoc does not return `""` for non-elements.** Its `stringify`
   (`external-sources/pandoc/pandoc-lua-engine/src/Text/Pandoc/Lua/Module/Utils.hs:341`)
   is `forcePeek . retrieving "stringifyable element" $ choice [...]`: a
   value matching none of the peekers (Pandoc, Inline, Block, Caption,
   Cell, Citation, TableHead, TableFoot, MetaValue) is an **error**. Only
   `Attr` and `ListAttributes` are explicitly mapped to `""`. q2's silent
   `""` for nil (pinned by `test_stringify_nil_value`) and for every
   unknown userdata (Citation, meta proxy, ListAttributes, config
   specials, Nodes) is already a divergence, just an unregistered one.
   Note that pandoc's `peekMetaValue` accepts booleans, numbers, strings
   and tables, so those are "elements" to pandoc too; nil is not.

## Dependency graph

- **discovered-from: bd-q4gnd5os** (in_progress, PR #859 open against
  `main`, not merged as of this investigation). That strand extends the
  fuzzy inline coercion so numbers and booleans read as text (divergence
  D-numtext). Its PR touches `nodes.rs` (constructor accepts scalars) but
  **not** `utils.rs`, so this fix does not conflict textually. It does
  affect which test inputs are legal: tests written here must build Nodes
  from strings/elements until #859 merges, or this branch must be based on
  #859's branch.
- **No blocks edges, no dependents.** Nothing is waiting on this; the
  pressure is "the test on #859 carries a workaround".
- **Related (not linked in braid yet): bd-0jtai5dh** "Five near-copies of
  pandoc stringify with subtly different semantics" (open, P3,
  2026-08-26). Lists `stringify_inline`/`stringify_inlines` in utils.rs as
  one of five plain-text renderers in the tree. Relevant constraint: the
  fix here should add a dispatch arm that *delegates* to the existing
  `stringify_inlines`/`stringify_blocks`, not a sixth renderer. Also worth
  knowing that q2's `stringify_block` and `stringify_blocks` already
  diverge from pandoc's `Shared.stringify` (q2 joins blocks with `"\n"`
  and emits CodeBlock text; pandoc concatenates with nothing and ignores
  CodeBlock, and renders SoftBreak/LineBreak as `" "` where q2 uses
  `"\n"`). Out of scope here, but the Nodes arm inherits whichever
  semantics those helpers have.

## What the code looks like today

All paths in the strand still exist with the described shape.

- `stringify_value` at `crates/pampa/src/lua/utils.rs:786` is the Lua
  entry point for `pandoc.utils.stringify` and is also `pub(crate)`,
  called from `crates/pampa/src/lua/json.rs:144` and `:196` (JSON
  encoding of Inlines/Blocks tables and of Inline/Block userdata). The
  json.rs userdata caller guards with `ud.is::<LuaInline>() ||
  ud.is::<LuaBlock>()` before calling, so adding a Nodes arm to
  `stringify_value` does **not** change `pandoc.json.encode(Nodes)` (still
  null) unless that guard is widened too.
- `LuaNodes` (`crates/pampa/src/lua/nodes.rs:54`) has
  `shape: NodesShape` with `Empty | Blocks(Vec<Block>) | Inlines(Vec<Inline>)`,
  plus `to_inlines()` (fallible) and `to_blocks(lua)` (infallible). The
  stringify arm needs neither resolution; it can match the shape directly
  and call `stringify_inlines` / `stringify_blocks`.
- The sibling `pandoc.utils` dispatchers, checked as the strand asked:
  - `type` (`utils.rs:288`): **already handles Nodes** (returns
    `"Nodes"`, pinned by `utils_type_and_tostring_name_it` in nodes.rs).
  - `blocks_to_inlines` (`utils.rs:78`) via `extract_blocks`
    (`utils.rs:122`): silently drops any userdata that is not a
    `LuaBlock`, so a Nodes argument yields an empty Inlines. Same shape
    of gap as stringify. The separator argument likewise ignores
    non-Inline userdata.
  - `equals` (`utils.rs:273`): delegates to Lua `==`. `LuaNodes` defines
    `__tostring` and `__len` but **no `__eq`**, so two structurally equal
    Nodes compare by identity and `equals` returns false. Pandoc has no
    Nodes type, so there is no parity answer; the question is whether Nodes
    should get the structural `__eq` that Inline/Block/Citation have
    (`types.rs:976`, `:1773`, `:2712`).
- The repro at HEAD lives in
  `claude-notes/plans/stringify-nodes-investigation/` (`filter.lua` +
  `doc.qmd`); it prints each case to stderr. Expected output before the
  fix: every Nodes case is `""`, the Span-wrapped case is `"42"`,
  `type` is `Nodes`, `blocks_to_inlines` yields 0 inlines, `equals` is
  false. See the investigation README for the run command and captured
  output.

## Proposed phases (draft)

Skeleton only; phase contents wait on the design discussion.

- Phase 0 — Tests first. Unit tests next to the existing `test_stringify_*`
  block in utils.rs (Inlines-shaped, Blocks-shaped, Empty, and a Nodes
  inside a table/Inlines list), plus one case in the nodes.rs test module.
  Build Nodes from strings/elements so the tests pass on `main` with or
  without #859.
- Phase 1 — Add the `LuaNodes` arm to `stringify_value`: match
  `NodesShape` and delegate to `stringify_inlines` / `stringify_blocks`
  (no new renderer, per bd-0jtai5dh).
- Phase 2 — Siblings, depending on Q2 below: `extract_blocks` accepts a
  Nodes via `to_blocks(lua)` (needs the `Lua` handle threaded in);
  optionally `__eq` on `LuaNodes`.
- Phase 3 — Fallthrough policy, depending on Q3 below: either leave the
  silent `""` and register it as a divergence in
  `crates/pampa/tests/lua-conformance/divergences.md`, or turn unknown
  userdata into an error matching pandoc's "stringifyable element".
- Phase 4 — After PR #859 merges: drop the `pandoc.Span(...)` wrapper in
  `numbers_and_booleans_read_as_text_in_inline_positions_only` (or do it
  on #859 if this lands first and #859 rebases).
- Phase 5 — Docs: a line in the Nodes section of the lmd docs if
  stringify is mentioned there; link bd-0jtai5dh as `related`.

## Open design questions for the user

1. **Sequencing against PR #859.** Keep this branch on `main` and write
   tests with string-built Nodes (recommended: the fix is independent and
   #859 does not touch utils.rs), or base it on #859's branch so the tests
   can use `pandoc.Nodes(42)` verbatim? Either way, who removes the Span
   workaround in the #859 test, and on which branch?
2. **Scope of the sibling fixes.** Should this strand also make
   `blocks_to_inlines` accept a Nodes (resolve via `to_blocks`), and give
   `LuaNodes` a structural `__eq` so `pandoc.utils.equals` and `==` work?
   Or file those as separate strands and keep this one to stringify?
3. **Unknown-userdata fallthrough.** Pandoc errors on anything it cannot
   peek; q2 returns `""` for nil, Citation, meta proxies, ListAttributes,
   config specials, and (today) Nodes. Options: (a) keep `""` and register
   the divergence; (b) error on unknown userdata but keep `""` for nil;
   (c) full parity, error on nil too. (b) is my recommendation: it would
   have surfaced this bug at the first test, and it does not break the
   nil case that is already pinned. Note (b) and (c) change behavior for
   Citation and meta proxies, which should get real arms rather than
   errors (pandoc stringifies a Citation, and a meta proxy is a MetaValue).
4. **Blocks-shaped Nodes rendering.** Delegating to `stringify_blocks`
   joins blocks with `"\n"` (q2's existing choice, not pandoc's). Is that
   acceptable for Nodes, or should a Nodes render the way its
   block-context resolution would (identical here, since `to_blocks`
   returns the blocks unchanged)? I assume delegate-and-inherit; flagging
   because bd-0jtai5dh may later change what `stringify_blocks` does.

## Risks / tradeoffs (draft)

- Small change, well-tested area (stringify has ~20 unit tests). Main risk
  is scope creep into bd-0jtai5dh's refactor; the plan avoids that by
  delegating.
- Widening the json.rs guard to include Nodes would change
  `pandoc.json.encode` output for Nodes from null to a string. Not
  proposed here; mention only so it is a deliberate non-change.
- If Q3 picks an error, any filter in the wild that stringifies a
  Citation or meta proxy today and gets `""` would start failing. Grep the
  docs and fixtures for `stringify(` on those before flipping.
