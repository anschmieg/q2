---
title: 'pandoc.OrderedList with default ListAttributes serializes as "Default"; pandoc rejects the JSON (bd-3h6iasmz)'
date: 2026-10-16
description: 'Spell ListNumberStyle::Default and ListNumberDelim::Default the way pandoc does (DefaultStyle, DefaultDelim) everywhere pampa writes them, from one canonical helper on the enums.'
status: in-progress  # PR #857 open 2026-10-16, awaiting CI and review
braid:
  strand: bd-3h6iasmz
---

**Branch:** `braid/bd-3h6iasmz-pandocorderedlist-default-listattributes-serializes` (topic branch in the main checkout, off `main` @ `0a64ffaab`)

## The bug

`pandoc.OrderedList(items)` with no explicit attributes gives a list whose
`ListAttributes` are `(1, Default, Default)`. The native HTML writer handles
that fine, so the lmd docs examples looked right. Every pandoc-leg format
(`--to markdown`, `docx`, `pdf`, ...) failed with `Q-20-3`:

```
JSON parse error: Error in $.blocks[0][1]: When parsing
Text.Pandoc.Definition.ListNumberStyle expected an Object with a tag field
where the value is one of [DefaultStyle, Example, Decimal, LowerRoman,
UpperRoman, LowerAlpha, UpperAlpha], but got Default.
```

Reproduced at `0a64ffaab` with one lmd cell returning a bare
`pandoc.OrderedList` and `q2 render --to markdown`.

## Where the spelling lived

Seven places spelled the names by hand, and the two JSON writers plus the
native writer got `Default` wrong in three different ways:

| site | `Default` style | `Default` delim |
|---|---|---|
| `pampa/src/writers/json.rs`, serde path | `Default` (wrong) | `Default` (wrong) |
| `pampa/src/writers/json.rs`, streaming path | `Default` (wrong) | `Default` (wrong) |
| `pampa/src/writers/native.rs` | `Decimal`, with a "is this the default?" comment | `Period`, same comment |
| `pampa/src/readers/json.rs` | `DefaultStyle` | `DefaultDelim` |
| `pampa/src/lua/constructors.rs` (parse and name) | `DefaultStyle` | `DefaultDelim` |
| `pampa/src/lua/show.rs` | `DefaultStyle` | `DefaultDelim` |
| `quarto-core/src/lmd/emit.rs` | `DefaultStyle` | `DefaultDelim` |

So pampa could not even read its own JSON back, and the lmd authoring
guide had to document "pass `pandoc.ListAttributes(1, "Decimal",
"Period")` explicitly" as a limitation.

## The fix

One canonical pair on the enums in `quarto-pandoc-types/src/list.rs`:
`pandoc_name()` and `from_pandoc_name()`, with an `ALL` constant so the
inverse is derived rather than spelled twice. Every site above delegates
to it. The native writer now prints `DefaultStyle` and `DefaultDelim`,
which is what pandoc itself prints for a list built with default
attributes.

## Work log

- [x] reproduce at HEAD; locate every site (table above)
- [x] `pandoc_name` / `from_pandoc_name` / `ALL` with a round-trip unit test
- [x] writers (JSON serde, JSON streaming, native), JSON reader, Lua constructors, `show`, lmd emitter delegate
- [x] test: a bare `pandoc.OrderedList` written to JSON and native carries `DefaultStyle` / `DefaultDelim`
- [x] docs: drop the explicit `ListAttributes` from `examples/lmd/03-lists`, the patterns fixture and the lmd page; drop the limitation bullet; regenerate `patterns.md.snap` (which goes through pandoc, so it pins the fix end to end)
- [x] pampa (5242), quarto-pandoc-types, quarto-core lmd (72) and quarto lmd snapshot tests green; clippy, fmt, `cargo xtask lint` clean; the repro renders `1.  a` / `2.  b` to markdown
- [x] PR against `main`: [#857](https://github.com/quarto-dev/q2/pull/857), opened 2026-10-16
- [ ] merged; plan status `done`, strand closed
