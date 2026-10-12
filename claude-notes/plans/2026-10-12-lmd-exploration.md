---
title: 'lmd ("lua+md"): a JSX-like Lua + Markdown mode for Quarto 2 (bd-lmd-exploration-jqv2nbl4)'
date: 2026-10-12
description: 'Exploration of a syntax that puts filter logic next to the markdown it acts on: executable lmd cells whose Lua can contain markdown fragment expressions, desugared into pandoc constructor calls. Spike 1 vendors a tree-sitter-lua fork with the fragment syntax and shows the desugaring in rendered output.'
status: in-progress
braid:
  strand: bd-lmd-exploration-jqv2nbl4
---

**Branch:** `braid/bd-lmd-exploration-jqv2nbl4-lmd-exploration` (topic branch in the main checkout, based on `main` @ `a0975a7cc`; no worktree).

## The idea

Pandoc filters are a good foundation for AST-directed document processing, but the filter ends up far from the markdown that wants it. The Quarto 1 shape:

````qmd
---
filters: [foo.lua]
stuff: [a, b, c, d]
---

::: foo
:::
````

with a separate `foo.lua` that matches `Div`s by class, reaches back into metadata, and hopes nobody else uses `.foo`.

lmd puts the two in one place:

````qmd
---
stuff: [a, b, c, d]
---

::: foo

```{lmd}
return tmap(quarto.metadata.get("stuff"), function(v)
  return {= **`{lmd} v`** =}
end)
```

:::
````

Two pieces of syntax:

1. **The `{lmd}` cell.** An executable cell whose body is Lua. The eventual design desugars it into a Lua filter that dispatches the cell's result to the cell's own position in the document, so the author never names a class to match on. Inline form `` `{lmd} expr` ``.
2. **The `{= markdown =}` expression.** Inside lmd Lua, a markdown fragment is an expression. The fragment is parsed with the qmd parser and becomes a Pandoc AST value, written out as `pandoc.*` constructor calls. Markdown inside a fragment can contain lmd code again, so the languages nest in both directions.

The result should approach the ergonomics of Hugo, Astro and Docusaurus templates while plain Lua filter extensions keep working alongside, because the whole thing is desugaring: the output is ordinary qmd plus ordinary Lua filters.

## Spike 1 (done 2026-10-12): the grammar and a visible desugaring

The goal of the first spike is to **read** the desugaring, not run it. A pipeline stage takes each `{lmd}` cell, desugars its `{= =}` fragments into `pandoc.*` calls, and leaves the result in the document as a non-executable ```` ```lmd ```` block so `q2 render --to markdown` shows it.

### What landed

- **`crates/tree-sitter-lmd`**: a vendored fork of [tree-sitter-grammars/tree-sitter-lua](https://github.com/tree-sitter-grammars/tree-sitter-lua) v0.5.0 (full Lua 5.4, plus the 5.5 `global` keyword), renamed to grammar `lmd` so it coexists with the upstream `tree-sitter-lua` crate `quarto-highlight` links. One rule added, `markdown_fragment`, hung off `expression`:

  ```
  markdown_fragment: '{=' markdown_content? '=}'
  ```

  All three tokens come from the external scanner. `{=` cannot begin any Lua construct (a table-constructor field never starts with `=`), so the token is unambiguous wherever an expression is valid; on a lone `{` the scanner returns false and the internal lexer's `{` (table constructor) wins. The content token is raw text up to the matching `=}`, balancing nested `{=`/`=}` pairs so a fragment whose markdown has inline lmd code with its own fragment still ends in the right place. Content keeps its whitespace; the Rust side trims.

  Layout mirrors `crates/tree-sitter-doctemplate`: `grammar/` holds the upstream sources (only `grammar.js`, `src/`, `test/corpus/`, `queries/`, `tree-sitter.json`, the license), `build.rs` and `src/lib.rs` are ours. `cargo xtask ts-test lmd` regenerates and tests. The corpus has 49 tests, 42 upstream and 7 new; the Rust crate has 3.

- **`quarto_core::lmd`**: `desugar_lua(src) -> Result<String, LmdError>`. Parses the Lua with tree-sitter-lmd (a parse error fails the whole cell, with a position), finds every `markdown_fragment` in source order, parses each fragment's markdown with `pampa::readers::qmd::read`, and splices in the emitted expression. Continuation lines are indented past the column the `{=` sat at, so the splice reads as one expression inside whatever Lua surrounded it.

  The emitter (`lmd/emit.rs`) covers every `Block` and `Inline` that has a pandoc constructor, with pandoc's argument order (`Link(content, target, title, attr)`, `Header(level, content, attr)`, ...). An empty `Attr` is omitted. Unsupported nodes (`Table`, note definitions, `Shortcode`, the CriticMarkup inlines, `Custom`) fail the cell with `LmdError::Unsupported`.

  The escape hatches back to Lua: an inline `` `{lmd} expr` `` (a `Code` whose text starts with `{lmd}`) becomes `expr` in the enclosing inline list, parsed as a Lua *expression* (wrapped in `return` for the parse) and desugared recursively; a block `` ```{lmd} `` cell inside a fragment becomes `(function() body end)()` in the enclosing block list.

- **`LmdDesugarStage`** (`stage/stages/lmd_desugar.rs`), placed right after `PreEngineSugaringStage` and before `EngineExecutionStage` in `build_html_pipeline_stages_with_options`, so every pipeline derived from it (HTML, the pandoc leg, `--to markdown`) gets it and no engine sees an `{lmd}` cell. A cell that fails to desugar is left untouched and reported as a warning.

### What the output looks like

The motivating example renders to:

```lua
return tmap(quarto.metadata.get("stuff"), function(v)
  return pandoc.Para({
           pandoc.Strong({
             v,
           }),
         })
end)
```

Nesting in both directions, from

`````qmd
````{lmd}
local function card(title, body)
  return {=
::: {.card}
### `{lmd} title`

```{lmd}
return body
```
:::
=}
end
return card("Hello", {= Some *body* text. =})
````
`````

renders to:

```lua
local function card(title, body)
  return pandoc.Div({
           pandoc.Header(3, {
             title,
           }, pandoc.Attr("lmd-title", {}, {})),
           (function()
             return body
           end)(),
         }, pandoc.Attr("", {
           "card",
         }, {}))
end
return card("Hello", pandoc.Para({
                       pandoc.Str("Some"),
                       pandoc.Space(),
                       pandoc.Emph({
                         pandoc.Str("body"),
                       }),
                       pandoc.Space(),
                       pandoc.Str("text."),
                     }))
```

Headings, bullet lists, links with titles, inline code, math, quotes, line breaks and attributes all come through (`scratchpad/lmd-demo.qmd` during the session; worth turning into a fixture in spike 2).

### Observations from reading it

1. **Block context is forced.** A fragment always produces a block: one paragraph becomes `pandoc.Para`, another single block becomes its own constructor, several become `pandoc.Blocks`. `{= **x** =}` therefore cannot sit inside `pandoc.Strong(...)`: there is no way to ask for inlines. See open question 1.
2. **Inline splices have no coercion contract.** `v` lands as a bare list element, so the Lua value must already be an `Inline` (or whatever pampa's `pandoc.Strong` accepts in a content list). A string works in pandoc proper by coercion to `Str`, but spaces would not become `Space`. Something like `lmd.inlines(v)` at the splice may be needed; it should be decided with question 1.
3. **Spliced headings get a derived id.** The qmd reader auto-generates `lmd-title` from the heading text `` `{lmd} title` `` before the splice happens. The desugaring should probably strip reader-derived ids (and anything else derived from text that is really a placeholder).
4. **Footnotes do not work yet.** The reader emits `NoteDefinitionPara` blocks that are resolved later in the pipeline, so a fragment with a footnote is `Unsupported`. Either resolve notes inside the fragment parse or run the fragment through the same early transforms the document gets.
5. **Fence nesting is on the author.** A cell containing a block-level `` ```{lmd} `` must itself use four backticks. Normal markdown, but it will surprise people.
6. **Inline lmd outside a fragment is untouched.** `` `{lmd} v` `` in a document paragraph (not inside a cell) is left as a `Code` node. Whether it should be an inline cell is a design question for the dispatch half.

## Delimiter design notes (2026-10-12)

Spike 1 uses one fragment form, `{= … =}`, and always produces blocks. The next step wants two forms, so the block-versus-inline context is explicit in the syntax, matching the pandoc habit that `[...]` means inline (spans, links) and `:::` means block (divs, the block-footnote extension). This section records what the Lua side allows, so the choice can be made later without re-deriving it.

### What can follow `{` in Lua

A fragment opener has to be a two-character sequence that no valid Lua program contains at an expression start, because the external scanner is consulted first wherever an expression can begin. After `{` (a table constructor) Lua accepts a field start: a name or `_`, a digit, a string quote, `[` (bracketed key or long string), `{`, `(`, `-`, `#`, `~`, `.` (for `...`), `}`, or a `--` comment. Every other ASCII punctuation character is free as the second character: `=` `:` `|` `!` `@` `$` `?` `<` `>` `/` `*` `^` `&` `%` `+` `,` `;` `` ` `` `\`.

So `{=` (current), `{:`, `{|`, `{!`, `{$`, and so on are all unambiguous openers. Once the first two characters are free, the third is unconstrained: `{|:` and `{|[` are as safe as `{|`.

### Why `{[ … ]}` does not work

`{[` is valid Lua twice over: a bracketed key, `{[k] = v}`, and a long string in a table, `{[[text]]}`. The scanner cannot rescue it with lookahead either, because ordinary code contains the closer: `{[k] = t[1]}` ends in `]}` and would lex as a fragment. Keep this example in mind whenever a `[`-led opener comes up again.

### `[: … :]` is available but reads wrong

An expression can begin with `[` only as a long string (`[[` or `[=`), and the index form `a[...]` is only valid after a prefix expression, where no expression can start. So `[:` never competes with existing Lua for the same lookahead. It was rejected on reading grounds: it combines the span character `[` with the div character `:`, so it has to be taught as *not* div-related.

### Closer collisions, measured

Counted over the 2139 `.qmd` and `.md` files under `claude-notes/` and `docs/`, counting lines containing the sequence anywhere (every hit is inside code or YAML, not prose; a fragment containing such a code block would collide with any delimiter equally):

| sequence | files | lines |
|---|---|---|
| `]}` | 78 | 164 |
| `:}` | 13 | 26 |
| `*}` | 5 | 7 |
| `:]` | 5 | 7 |
| `|}` | 1 | 1 |
| `:|}` `]|}` `:=}` `]=}` | 0 | 0 |

`]}` is the worst closer on the markdown side as well as the Lua side.

### Candidates on the table

| blocks | inlines | expands to | notes |
|---|---|---|---|
| `{: … :}` | `[: … :]` | `pandoc.Blocks`, `pandoc.Inlines` | shortest; `[:` has the teaching problem above |
| `{|: … :|}` | `{|[ … ]|}` | same | a discriminator `|` then the context character; zero corpus collisions; more typing; OCaml quoted strings `{| … |}` and `{id| … |id}` are a precedent for the shape |
| `{=: … :=}` | `{=[ … ]=}` | same | same idea with `=`, keeping continuity with spike 1; zero corpus collisions |

Whichever pair is chosen, nested pairs balance in the scanner the way `{=`/`=}` do today, and `Para` versus `Plain` stops being a syntax question: the block form yields whatever the qmd reader produces (a top-level paragraph is `Para`), and an explicit `pandoc.Plain([: … :])` covers the rest. The inline form should reject markdown that parses to more than one block, since the author asked for inlines.

## Open questions

1. Block vs inline result context for `{= =}`. Decided in principle: two delimiter pairs, one per context (see the delimiter design notes above). The exact characters are still open.
2. The splice coercion contract for inline and block lmd code inside a fragment (observation 2).
3. The dispatch half: how an `{lmd}` cell's result replaces the cell at its own AST position, and how the desugared Lua becomes a filter that existing Lua filters compose with. Also whether the cell runs at render time only or has a preview-time story.
4. Error locations. `LmdError` carries a position inside the cell text; the warning does not yet map it back to the document.
5. The name.

## Next spikes

- **Spike 2: run it.** Feed the desugared Lua to pampa's Lua runtime for a cell whose fragments use only immediate values, and replace the cell with the result. This is the smallest version of the dispatch half and will settle questions 1 and 2 by forcing them.
- Turn `lmd-demo.qmd` into an integration fixture with a snapshot of the `--to markdown` output.
- Decide the inline-context syntax (question 1) once spike 2 shows what the Lua side needs.

## Verification (spike 1)

- `cargo xtask ts-test lmd`: 49 of 49 corpus tests.
- `cargo nextest run -p quarto-core -E 'test(lmd)'`: 14 of 14.
- `cargo test -p xtask tree_sitter`, `cargo clippy -p quarto-core -p tree-sitter-lmd -p xtask --all-targets -- -D warnings`, `cargo xtask lint`: green.
- WASM: not built locally in this session; the crate follows `tree-sitter-doctemplate`\'s build exactly, and `quarto-core` already pulls two other C-backed grammars into the wasm32 tree. CI's build-wasm job is the check.
