---
title: 'lmd: inline cells reject numbers; meta.version (YAML number) needs tostring() (bd-iqd6niat)'
date: 2026-10-16
description: 'Decide where a Lua number (and boolean) becomes text when an lmd inline cell or a pandoc constructor is handed one, now that q2 metadata exposes YAML numbers as real Lua numbers (divergence D-num).'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-iqd6niat
---

**Branch:** `braid/bd-iqd6niat-lmd-inline-numbers` (topic branch in the main checkout, based on `main` @ `e308835bd`)
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The symptom reproduces at HEAD, the code is small
and well-tested, and the fix is a one-arm change in whichever coercion
layer we pick. What is *not* settled is the design: the strand's
proposal ("accept a number in pampa's fuzzy inline coercion, so
`pandoc.Strong(3)` agrees") would make pampa accept something pandoc
itself rejects. The questions below are about where the number→text
step belongs and whether booleans join it.

## Issue context

Filed 2026-10-15 by claude while smoke-testing the lmd authoring-guide
examples (bd-lmd-docs-41adv2nx) on main (q2 0.33.0). With
`version: 12.1` in front matter, `` `{lmd} meta.version` `` in a
paragraph fails:

> an inline lmd cell must evaluate to an Inline, a list of Inlines, a
> string, or nil; this one evaluated to a number

The author has to write `tostring(meta.version)`. Numbers (and
booleans) are the one metadata leaf type the inline contract does not
accept. The strand also notes that `{{{< meta version >}}}` with a YAML
number "expanded to nothing in a heading" (spike 5 of the exploration
plan, "not investigated").

Open, priority 3, type `feature`, labels `lmd`, `lua`. One day old;
no stale assumptions.

## Dependency graph

```
bd-iqd6niat
  ├─ discovered-from → bd-lmd-docs-41adv2nx        (in_progress) docs: lmd authoring guide
  └─ related         → bd-lmd-exploration-jqv2nbl4 (in_progress) lmd exploration, PR #823
```

- **discovered-from** bd-lmd-docs-41adv2nx: the docs guide wants a
  "reach for metadata and compute on it" section whose first example
  is `meta.version`. The guide will tell readers to use `tostring()`
  until this strand is decided, so the docs page is the consumer with
  a deadline. Its plan file (`claude-notes/plans/2026-10-15-lmd-docs.md`)
  lives on the docs branch, not on `main`; not read here.
- **related** bd-lmd-exploration-jqv2nbl4: spike 3 defined the inline
  cell contract and *deliberately* listed "a number" among the error
  cases (plan line 281: "A `Block`, `Blocks`, a number, or a function
  is an error naming the value"). Spike 6 added `meta` /
  `quarto.metadata.get` returning "numbers and booleans are themselves".
  The two spikes are individually right and jointly produce this
  strand. Spike 5's "noticed on the way" note is the shortcode
  sibling bug.
- **No incoming `blocks` edges.** Nothing is waiting on this except the
  docs wording.

### The decision this strand sits on top of: divergence D-num

`crates/pampa/tests/lua-conformance/divergences.md` § "Meta numbers
are first-class (D-num)", reviewed by Carlos 2026-07-20
(bd-2llqjsms / bd-a9g50za2, plan
`claude-notes/plans/2026-07-20-lua-meta-pandoc-filters.md`):

> metadata numbers are never stringified. Pandoc's `peekMetaValue`
> renders Lua numbers to `MetaString` ("5", "13.37"); q2 keeps them as
> `Scalar(Integer)`/`Scalar(Real)`, and they read back from `meta` as
> Lua numbers. \[...\] `tostring(meta.count)` still renders "3" where
> filters format text.

So `tostring()` being needed is the *documented* consequence of
D-num. Pandoc never has this problem because its YAML reader turns
`version: 12.1` into `MetaInlines [Str "12.1"]` (verified with pandoc
3.11 below). The strand is really asking: now that lmd makes "put a
metadata value in prose" a one-token operation, is the D-num cost
worth softening at the inline boundary?

## What the code looks like today

All paths in the description still exist; nothing has been refactored
since the strand was filed yesterday.

- **Inline cell contract**: `classify_inline_value` in
  `crates/pampa/src/lua/lmd.rs` (around line 254). `Value::String`
  goes through `peek_inlines_fuzzy`; `Value::Number` / `Integer` /
  `Boolean` fall to the `other =>` arm and become
  `LmdCellError::NotInlines { got: "a number" }`. List elements are
  classified the same way (a number inside `{ "v", meta.version }`
  is "a list whose element 2 is a number"). The block contract
  (`classify_cell_value`) rejects numbers too, correctly: a bare number
  has no block meaning.
- **Pinned by tests**: `inline_cells_reject_blocks_and_other_values`
  (lmd.rs ~line 489) asserts `"42"` → error containing "a number";
  `inlines_an_inline_and_strings_are_rejected_not_wrapped` does the
  same for block cells. Both would flip if numbers are accepted.
- **Fuzzy coercion layer**: `peek_inline_fuzzy` / `peek_inlines_fuzzy`
  in `crates/pampa/src/lua/types.rs` (~lines 2058, 2128) accept only
  string / userdata / table. `test_peek_inlines_fuzzy_error_on_number`
  and `test_peek_blocks_fuzzy_error_on_number` (types.rs ~4799, ~4900)
  pin the rejection, introduced with the layer in commit `66016256c`
  "Implement Pandoc-compatible fuzzy type coercion". 68 call sites use
  the inlines variant (every constructor content argument, filter
  return values, `pandoc.Inlines(...)`).
- **Diagnostic**: `failure_diagnostic` in
  `crates/quarto-core/src/stage/stages/lmd_execute.rs` (~line 262)
  titles it "inline lmd cell did not produce inlines" with a hint
  pointing at `{=[ … ]=}`. The hint does not mention `tostring()`.
- **Metadata leaf mapping**: `push_yaml_scalar` in
  `crates/pampa/src/lua/config_value.rs` (~line 179): Integer →
  `Value::Integer`, Real → `Value::Number` when it parses (else the raw
  string), Boolean → `Value::Boolean`, Null → nil. This is D-num.
- **Existing `tostring`-free escape hatch**: `pandoc.utils.stringify`
  already accepts numbers and booleans (`utils.rs` ~line 812), so
  `pandoc.utils.stringify(meta.version)` works today as well as
  `tostring(meta.version)`.

### What pandoc 3.11 does (reference points for "agreeing with pandoc")

Run locally with `pandoc lua -e`:

| expression | pandoc 3.11 |
|---|---|
| `pandoc.Str(3)` | `Str "3"` (hslua's `peekText` accepts a number as "stringy") |
| `pandoc.Str(1.5)` | `Str "1.5"` |
| `pandoc.Str(2^53)` | `Str "9.007199254741e+15"` (Lua `%.14g` formatting) |
| `pandoc.Str(true)` | error: string expected, got boolean |
| `pandoc.Strong(3)` | error: Inline, list of Inlines, or string expected, got number |
| `pandoc.Inlines(3)` | same error |
| `pandoc.Para(3)` | same error |
| `pandoc.Strong(true)` | error: ..., got boolean |
| YAML `version: 12.1` | `MetaInlines [Str "12.1"]` (never a number) |
| YAML `flag: true` | `MetaBool True` |

So **pandoc's fuzzy inline layer rejects numbers**; only the
`text`-typed argument of `pandoc.Str` coerces. The strand's "so
`pandoc.Strong(3)` agrees" is therefore backwards as a conformance
argument: making pampa accept `pandoc.Strong(3)` would be a *new*
divergence, not the removal of one. (It might still be the right call;
that is design question 1.)

### Repro at HEAD

Fixture: `claude-notes/plans/lmd-inline-numbers-investigation/repro.qmd`.
Command and captured output:
`claude-notes/plans/lmd-inline-numbers-investigation/repro-output.txt`.

Rendered on `main` @ `e308835bd` (fixture copied outside `claude-notes/`,
which is a project that excludes it from its render list). Result:
**6 errors, all "inline lmd cell did not produce inlines"**, each
located on the right code span, and the shortcode sibling bug in the
heading.

| source | stderr | rendered |
|---|---|---|
| `# Heading {{{< meta version >}}} and {{{< meta count >}}}` | (silent) | `<h1>Heading  and 3</h1>`: the real `12.1` vanished, the integer `3` survived |
| `` `{lmd} meta.version` `` | evaluated to a number | code span left displayed |
| `` `{lmd} meta.count` `` | evaluated to a number | code span left displayed |
| `` `{lmd} meta.flag` `` | evaluated to a boolean | code span left displayed |
| `` `{lmd} tostring(meta.version)` `` | ok | `Workaround: 12.1` |
| `` `{lmd} 42` `` | evaluated to a number | code span left displayed |
| `` `{lmd} meta.count * 2` `` | evaluated to a number | code span left displayed |
| `` `{lmd} {"v", meta.version}` `` | a list whose element 2 is a number | code span left displayed |

The hint printed under each error points at `{=[ … ]=}` and does not
mention `tostring()`, so an author who hits this has no in-band path
to the workaround.

### Sibling bug: `{{{< meta version >}}}` with a YAML real

`config_value_to_inlines` in
`crates/quarto-core/src/transforms/shortcode_resolve.rs` (~line 358)
handles string, bool and **integer** scalars explicitly, then falls to
a `Scalar { .. }` arm that calls `ConfigValue::as_plain_text()`, which
returns `None` for `Yaml::Real`
(`crates/quarto-pandoc-types/src/config_value.rs` ~line 830). The
fallback synthesises an **empty `Str`**, which is exactly spike 5's
"expanded to nothing". Integers work; reals and nulls vanish silently.
This is independent of Lua and of lmd; filed separately as
**bd-32nrymty** (`discovered-from` this strand) rather than folded in here,
because its fix (add a `Yaml::Real` arm, probably in `as_plain_text`
itself so every caller benefits) has no design question attached.

## Proposed phases (draft)

Skeleton only; which phase 1 we do depends on design question 1.

- **Phase 0 — Test plan.** Flip the two pinned "a number is an error"
  assertions in `lmd.rs` into positive tests (number → `Str`, number in
  a list, float formatting, integer formatting, `meta.version` /
  `meta.count` end to end through `LmdExecuteStage`). Add a stderr /
  HTML snapshot for the docs-guide example. If option (a) below: also
  flip `test_peek_inlines_fuzzy_error_on_number` and add constructor
  tests (`pandoc.Strong(3)`, `pandoc.Inlines{ "a", 3 }`), plus a
  conformance-divergence entry.
- **Phase 1 — The coercion change.** One of:
  - (a) *fuzzy layer*: `Value::Integer | Value::Number` arm in
    `peek_inline_fuzzy` and `peek_inlines_fuzzy` → `Str` via Lua
    `tostring` semantics; register divergence **D-numtext** in
    `divergences.md` next to D-num ("because q2 metadata hands filters
    numbers, q2 constructors accept them where pandoc's would error").
    Block layer (`peek_blocks_fuzzy`) stays strict or goes `Plain{Str}`
    — same question as strings do today.
  - (b) *lmd-only*: a `Value::Integer | Value::Number` arm in
    `classify_inline_value` (and in its list-element loop) that formats
    and delegates to the string path; constructors unchanged and still
    pandoc-conformant. Block cells keep rejecting numbers.
  - (c) *no coercion; better diagnostic*: keep the error, extend the
    hint with "wrap a number in `tostring()`", and let the docs say so.
- **Phase 2 — Booleans.** Either the same arm (`"true"` / `"false"`),
  or an explicit error that names `tostring()`; see question 2.
- **Phase 3 — Diagnostic.** Whatever is still an error after phases 1–2
  gets the `tostring()` hint in `failure_diagnostic`.
- **Phase 4 — Docs.** Update the lmd authoring guide's metadata section
  (owned by bd-lmd-docs-41adv2nx; coordinate so the "use `tostring()`
  for now" sentence is removed or kept), and the `divergences.md`
  entry if (a).

## Open design questions for the user

1. **Where does the number become text?** Three candidates, in
   decreasing blast radius:
   - (a) pampa's fuzzy inline coercion (`peek_inline(s)_fuzzy`), so
     `pandoc.Strong(meta.count)`, `pandoc.Inlines{ "n = ", meta.count }`
     and filter returns all accept numbers. Consistent story
     ("D-num gives you numbers, every inline sink takes them"), but it
     is a **new divergence from pandoc** (pandoc rejects
     `pandoc.Strong(3)`), touches 68 call sites\' behaviour, and needs a
     D-numtext entry plus xfail bookkeeping if any conformance test
     covers it.
   - (b) only the lmd inline-cell contract (`classify_inline_value`),
     so `` `{lmd} meta.version` `` works but `pandoc.Strong(meta.count)`
     still errors like pandoc. Smallest change; the lmd contract is
     already q2-specific so no conformance cost. The asymmetry
     ("the cell takes a number, the constructor inside it doesn't") is
     the cost.
   - (c) no coercion; keep `tostring()` as the documented spelling and
     improve the hint. Zero semantic change; the docs page says
     "metadata numbers are numbers; format them with `tostring()`".
   My recommendation is **(b)**: it is the lmd-native fix for an
   lmd-native pain, and it leaves the pandoc-conformance surface alone.
   If you want (a), the D-num review precedent suggests it should be
   logged as a reviewed divergence rather than slipped in.
2. **Booleans.** `meta.flag` is a Lua boolean. Should an inline cell
   render it as `true` / `false` (Lua `tostring`), or stay an error
   with a `tostring()` hint? Pandoc rejects booleans everywhere
   including `pandoc.Str(true)`; the shortcode `{{{< meta flag >}}}`
   renders `true`. Rendering it is the consistent-with-shortcodes
   answer; erroring is the "a boolean in prose is almost always a
   bug" answer. I lean to **render** so the lmd inline cell and the
   `meta` shortcode agree on every scalar leaf.
3. **Number formatting.** Lua `tostring` gives `3`, `12.1`, and
   `1e+15` for large floats, and `3.0` for a float that happens to be
   integral (`meta.x` from YAML `x: 3.0` is `Value::Number(3.0)`).
   Is "whatever Lua's `tostring` says" acceptable (it is what the
   author would get from writing `tostring()` by hand, so it is the
   least surprising), or should YAML reals keep their source spelling?
   The latter needs the raw YAML text, which `push_yaml_scalar` throws
   away, so I propose Lua `tostring` semantics and note it in the docs.
4. **Block cells and numbers.** A block cell returning `42` is an error
   today and pandoc-wise a block context has no number meaning. Leave
   as is (recommended), or wrap as `Plain{Str}` the way a bare string
   would be if the block contract allowed strings (it does not)?
5. **Docs wording timing.** The docs guide is in flight. Should it ship
   with the `tostring()` sentence and be edited after this lands, or
   should this land first so the guide shows the bare form? Only
   matters for sequencing the two branches.

## Risks / tradeoffs (draft)

- **Option (a) silently changes constructor semantics** for every Lua
  filter running under q2. A filter that relied on
  `pandoc.Inlines(n)` erroring (unlikely) would change behaviour; more
  realistically, a filter bug that today surfaces as a type error
  would render a number instead. Low risk, but it is the kind of
  divergence the 2026-07-20 review wanted logged explicitly.
- **Option (b) creates a two-tier contract**: the cell boundary is
  lenient, constructors inside the cell are strict. The docs need one
  sentence about it.
- **Float formatting via `tostring`** is Lua-version- and
  platform-stable (`%.14g`), but `3.0` → `"3.0"` may surprise authors
  whose YAML said `3.0` expecting `3`. Pandoc avoids this by never
  producing a number. Document it.
- **Test flips**: two pinned negative tests in `lmd.rs` and (for option
  (a)) two in `types.rs` become positive. That is expected, not a
  regression signal, but the reviewer should see it called out.
- **The shortcode sibling bug is a real silent data loss** (a float in
  `{{{< meta >}}}` renders as nothing, no warning). It is filed
  separately; it should probably be fixed first since it is a
  one-arm change with no design question.
