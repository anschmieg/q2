---
title: 'lmd/pampa: numbers and booleans read as text in inline positions (bd-q4gnd5os)'
date: 2026-10-16
description: 'Option (a) of bd-iqd6niat: extend the fuzzy inline peekers so every inline sink of the pandoc API accepts a Lua number or boolean as its tostring text, registered as divergence D-numtext. Block positions stay strict.'
status: in-progress  # PR #859 open 2026-10-16, awaiting CI and review
braid:
  strand: bd-q4gnd5os
---

**Branch:** `braid/bd-q4gnd5os-lmdpampa-accept-lua-numbers` (topic branch in the main checkout, off `main` @ `5dfbce166`)

## Why

bd-iqd6niat (PR #852) made the lmd inline-cell boundary accept numbers
and booleans by formatting them as `tostring` would. That left one seam,
which the lmd authoring guide had to explain in three places: a
`pandoc` constructor argument still rejected them, so
`pandoc.Strong(meta.version)` failed, and so did an inline cell *inside
a fragment*, because the fragment emitter splices the cell into the
`pandoc.Para(...)` call the paragraph becomes:

```
runtime error: Q-11-3: Inline userdata, string, or Inline-like value expected, got number
```

Pandoc's strictness here costs nothing in pandoc, which never hands a
filter a Lua number from metadata. q2 does (divergence D-num), so the
most common metadata pattern failed with a type error.

## Decisions

Recorded on the strand by Carlos (2026-10-16) when option (a) was split
out of bd-iqd6niat; applied here as written:

1. **One rule, in the peeker.** `peek_inline_fuzzy` and
   `peek_inlines_fuzzy` accept `Integer`, `Number` and `Boolean` and
   read them as the string `tostring` gives (`lua_tolstring` through
   mlua's `coerce_string`; booleans spelled `true` / `false`), then
   apply the string rule. Everything downstream (constructor content
   arguments, `pandoc.Inlines`, filter return values, `pandoc.Nodes`,
   the lmd cell boundary) inherits it.
2. **Block positions stay strict.** `peek_block_fuzzy` and
   `peek_blocks_fuzzy` fall back to the inline peeker for strings; they
   now refuse a bare number or boolean before that fallback, so
   `pandoc.Div(3)` and `pandoc.Blocks({3})` error as in pandoc.
3. **Error messages keep pandoc's wording** ("Inline, list of Inlines,
   or string expected, got ..."), which the conformance suite matches
   on.
4. **Registered as D-numtext** in `tests/lua-conformance/divergences.md`,
   next to D-num. No upstream conformance case asserts the rejection of
   a number in an inline position, so `xfail.txt` is unchanged.

## Work log

- [x] `is_text_scalar` / `text_scalar_to_string` in `types.rs`; arms in both inline peekers; guards in both block peekers; `peek_inlines_fuzzy` test flipped, `peek_blocks_fuzzy` strictness test widened
- [x] `pandoc.Nodes` follows (bare scalar word-split, element scalar one `Str`); its rejection test now uses functions
- [x] `lmd.rs` drops its private `scalar_text` and delegates; module doc updated
- [x] tests: constructors (`pandoc.Strong(3)`, mixed `pandoc.Inlines`, `pandoc.Nodes`, bare-vs-`tostring` agreement, block strictness), filter return (`Str` handler returning `12.1`)
- [x] divergence registry entry D-numtext
- [x] docs: lmd page (metadata gotcha, the `tostring(i)` sentence, the limitation bullet), `Q-26-2` / `Q-26-6` examples, `lua-filters.qmd` D-num bullet; examples 03 and 06 and the patterns fixture drop `tostring()`
- [x] pampa suite (5084, Lua conformance included; four tests that pinned the old rejection flipped to functions or to the new text), quarto-core lmd (83), quarto lmd snapshots (unchanged output); clippy, fmt, `cargo xtask lint` clean; the lmd page, `Q-26-6`, `lua-filters.qmd` and this plan render under `--strict`; examples 03 and 06 render `Member number 1` and `version 0.4` from bare cells
- [x] PR against `main`: [#859](https://github.com/quarto-dev/q2/pull/859), opened 2026-10-16
- [ ] merged; plan status `done`, strand closed
