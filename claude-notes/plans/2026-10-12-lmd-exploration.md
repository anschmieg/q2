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
  return {=: **`{lmd} v`** :=}
end)
```

:::
````

Two pieces of syntax:

1. **The `{lmd}` cell.** An executable cell whose body is Lua. The eventual design desugars it into a Lua filter that dispatches the cell's result to the cell's own position in the document, so the author never names a class to match on. Inline form `` `{lmd} expr` ``.
2. **The markdown fragment expression.** Inside lmd Lua, a markdown fragment is an expression. The fragment is parsed with the qmd parser and becomes a Pandoc AST value, written out as `pandoc.*` constructor calls. Three forms: `{=: … :=}` for blocks, `{=[ … ]=}` for inlines, and the unmarked `{= … =}` whose context is meant to be decided by the caller (see the delimiter design notes). Markdown inside a fragment can contain lmd code again, so the languages nest in both directions.

The result should approach the ergonomics of Hugo, Astro and Docusaurus templates while plain Lua filter extensions keep working alongside, because the whole thing is desugaring: the output is ordinary qmd plus ordinary Lua filters.

## Spike 1 (done 2026-10-12): the grammar and a visible desugaring

The goal of the first spike is to **read** the desugaring, not run it. A pipeline stage takes each `{lmd}` cell, desugars its markdown fragments into `pandoc.*` calls, and leaves the result in the document as a non-executable ```` ```lmd ```` block so `q2 render --to markdown` shows it.

Spike 1 first shipped with a single `{= … =}` form that always produced blocks; the same day it was revised to the three forms decided under "Delimiter design notes" below. The description that follows is the revised state.

### What landed

- **`crates/tree-sitter-lmd`**: a vendored fork of [tree-sitter-grammars/tree-sitter-lua](https://github.com/tree-sitter-grammars/tree-sitter-lua) v0.5.0 (full Lua 5.4, plus the 5.5 `global` keyword), renamed to grammar `lmd` so it coexists with the upstream `tree-sitter-lua` crate `quarto-highlight` links. One rule added, `markdown_fragment`, hung off `expression`, in three forms:

  ```
  markdown_fragment: '{='  markdown_content? '=}'
                   | '{=:' markdown_content? ':=}'
                   | '{=[' markdown_content? ']=}'
  ```

  All nine tokens come from the external scanner; the Rust side tells the forms apart by the node kind of the `start` field. `{=` cannot begin any Lua construct (a table-constructor field never starts with `=`), so the openers are unambiguous wherever an expression is valid; on a lone `{` the scanner returns false and the internal lexer's `{` (table constructor) wins. The content token is raw text up to this form's closer, balancing nested fragments of any form so a fragment whose markdown has inline lmd code with its own fragment still ends in the right place. Content keeps its whitespace; the Rust side trims.

  Layout mirrors `crates/tree-sitter-doctemplate`: `grammar/` holds the upstream sources (only `grammar.js`, `src/`, `test/corpus/`, `queries/`, `tree-sitter.json`, the license), `build.rs` and `src/lib.rs` are ours. `cargo xtask ts-test lmd` regenerates and tests. The corpus has 53 tests, 42 upstream and 11 new; the Rust crate has 3.

- **`quarto_core::lmd`**: `desugar_lua(src) -> Result<String, LmdError>`. Parses the Lua with tree-sitter-lmd (a parse error fails the whole cell, with a position), finds every `markdown_fragment` in source order, parses each fragment's markdown with `pampa::readers::qmd::read`, and splices in the emitted expression: `pandoc.Blocks({ … })` for `{=: :=}`, `pandoc.Inlines({ … })` for `{=[ ]=}` (the markdown must parse to one paragraph or nothing, else `LmdError::NotInlines`), and `pandoc.Nodes({ … })` for the unmarked form. Continuation lines are indented past the column the opener sat at, so the splice reads as one expression inside whatever Lua surrounded it.

- **`pandoc.Nodes`** exists in pampa's Lua API as a stub that raises an error naming the two marked forms, so the desugared Lua is well-formed but an unmarked fragment cannot run until the context-deferred semantics are designed.

  The emitter (`lmd/emit.rs`) covers every `Block` and `Inline` that has a pandoc constructor, with pandoc's argument order (`Link(content, target, title, attr)`, `Header(level, content, attr)`, ...). An empty `Attr` is omitted. Unsupported nodes (`Table`, note definitions, `Shortcode`, the CriticMarkup inlines, `Custom`) fail the cell with `LmdError::Unsupported`.

  The escape hatches back to Lua: an inline `` `{lmd} expr` `` (a `Code` whose text starts with `{lmd}`) becomes `expr` in the enclosing inline list, parsed as a Lua *expression* (wrapped in `return` for the parse) and desugared recursively; a block `` ```{lmd} `` cell inside a fragment becomes `(function() body end)()` in the enclosing block list.

- **`LmdDesugarStage`** (`stage/stages/lmd_desugar.rs`), placed right after `PreEngineSugaringStage` and before `EngineExecutionStage` in `build_html_pipeline_stages_with_options`, so every pipeline derived from it (HTML, the pandoc leg, `--to markdown`) gets it and no engine sees an `{lmd}` cell. A cell that fails to desugar is left untouched and reported as a warning.

### What the output looks like

The motivating example renders to:

```lua
return tmap(quarto.metadata.get("stuff"), function(v)
  return pandoc.Blocks({
           pandoc.Para({
             pandoc.Strong({
               v,
             }),
           }),
         })
end)
```

The three forms side by side, from

````qmd
```{lmd}
local unmarked = {= **hello** =}
local blocks = {=: **hello** :=}
local inlines = {=[ **hello** ]=}
local strong = pandoc.Strong({=[ *inner* ]=})
```
````

render to:

```lua
local unmarked = pandoc.Nodes({
                   pandoc.Para({
                     pandoc.Strong({
                       pandoc.Str("hello"),
                     }),
                   }),
                 })
local blocks = pandoc.Blocks({
                 pandoc.Para({
                   pandoc.Strong({
                     pandoc.Str("hello"),
                   }),
                 }),
               })
local inlines = pandoc.Inlines({
                  pandoc.Strong({
                    pandoc.Str("hello"),
                  }),
                })
local strong = pandoc.Strong(pandoc.Inlines({
                               pandoc.Emph({
                                 pandoc.Str("inner"),
                               }),
                             }))
```

Nesting in both directions, from

`````qmd
````{lmd}
local function card(title, body)
  return {=:
::: {.card}
### `{lmd} title`

```{lmd}
return body
```
:::
:=}
end
return card({=[ Hello ]=}, {=: Some *body* text. :=})
````
`````

renders to:

```lua
local function card(title, body)
  return pandoc.Blocks({
           pandoc.Div({
             pandoc.Header(3, {
               title,
             }, pandoc.Attr("lmd-title", {}, {})),
             (function()
               return body
             end)(),
           }, pandoc.Attr("", {
             "card",
           }, {})),
         })
end
return card(pandoc.Inlines({
              pandoc.Str("Hello"),
            }), pandoc.Blocks({
                             pandoc.Para({
                               pandoc.Str("Some"),
                               pandoc.Space(),
                               pandoc.Emph({
                                 pandoc.Str("body"),
                               }),
                               pandoc.Space(),
                               pandoc.Str("text."),
                             }),
                           }))
```

Headings, bullet lists, links with titles, inline code, math, quotes, line breaks and attributes all come through (`scratchpad/lmd-demo.qmd` during the session; worth turning into a fixture in spike 2).

### Observations from reading it

1. **Context is now explicit.** `{=[ … ]=}` can sit inside `pandoc.Strong(...)` and `{=: … :=}` inside `pandoc.Div(...)`; the unmarked form waits on `pandoc.Nodes`. (The first cut of spike 1 forced block context; the delimiter design notes record how that was resolved.)
2. **Inline splices have no coercion contract.** `v` lands as a bare list element, so the Lua value must already be an `Inline` (or whatever pampa's `pandoc.Strong` accepts in a content list). A string works in pandoc proper by coercion to `Str`, but spaces would not become `Space`. Something like `lmd.inlines(v)` at the splice may be needed; `pandoc.Nodes` is the natural home for the same question.
3. **Spliced headings get a derived id.** The qmd reader auto-generates `lmd-title` from the heading text `` `{lmd} title` `` before the splice happens. The desugaring should probably strip reader-derived ids (and anything else derived from text that is really a placeholder).
4. **Footnotes do not work yet.** The reader emits `NoteDefinitionPara` blocks that are resolved later in the pipeline, so a fragment with a footnote is `Unsupported`. Either resolve notes inside the fragment parse or run the fragment through the same early transforms the document gets.
5. **Fence nesting is on the author.** A cell containing a block-level `` ```{lmd} `` must itself use four backticks. Normal markdown, but it will surprise people.
6. **Inline lmd outside a fragment is untouched.** `` `{lmd} v` `` in a document paragraph (not inside a cell) is left as a `Code` node. Whether it should be an inline cell is a design question for the dispatch half.
7. **Continuation indent is computed from the source column.** When two fragments share a line and the first expands to several lines, the second is still indented to its *source* column (see the `card(...)` call above). Cosmetic; fixing it means tracking the output column during splicing.

## Spike 2 (done 2026-10-12): running the cells

The desugared Lua now runs. A second stage, `LmdExecuteStage`, sits after engine execution and right before `user-filters-pre`; it runs every cell `LmdDesugarStage` marked, in document order, in one Lua state per document, and splices the returned blocks in place of the cell.

### Decisions

Taken as proposed on 2026-10-12, with the user's explicit yes on the last two:

1. **A cell may return a `Block`, a list of `Block`s, or `nil`** (the cell vanishes). `Inlines`, a single `Inline`, and strings are errors that name what came back, not wrapped in a `Plain`: a cell is block context, and `{=: … :=}` already says "blocks". Multiple return values are ignored past the first.
2. **One Lua state per document.** Globals set by one cell are visible to later cells, like a notebook. Cells run in pre-order document order, so a cell inside a div runs before a top-level cell that follows the div.
3. **After the engine, before user pre-filters.** `LmdDesugarStage` (pre-engine) rewrites the cell in place, drops the `{lmd}` class for a plain `lmd` one, and sets `lmd="pending"`; an unbraced class is plain code to every engine, so knitr and jupyter never see a cell. `LmdExecuteStage` finds the marked cells.
4. **Metadata stays out.** Q1's `quarto.metadata` (quarto-cli PR 13246; see `external-sources/quarto-cli/src/resources/lua-types/quarto/metadata.lua`) is not ported to q2 yet, and `quarto.doc.meta` is listed as unsupported in pampa. The spike runs in immediate mode. Porting it is its own strand when wanted.
5. **Echo is a cell option**, `--| echo: true` in Lua comment syntax, off by default. It keeps the (option-stripped) desugared Lua as a plain `lmd` code block above the result.
6. **A failing cell is an error diagnostic, located at the cell**, and the cell stays in the document as that displayed block, so the rest of the page renders and the failing Lua can be read. Three failure kinds: the chunk does not compile or raises, or it returns the wrong kind of value (the hint names the three accepted shapes and `{=: … :=}`).
7. **Inline `{lmd}` code in ordinary paragraphs stays out of scope** for this spike. (Done in spike 3, below.)

### What landed

- **`pampa::lua::lmd`**: `LmdRuntime` wraps the same Lua environment a user filter gets (`create_filter_environment`: `pandoc.*`, `quarto.*`, `FORMAT`, scoped `require`), with the document path standing in for the script path. `run_cell(source, chunk_name)` loads the chunk as a block (`call_async`, like filters), applies the return contract (`LmdCellValue::{Nothing, Blocks}` / `LmdCellError::{Lua, NotBlocks}`), and `take_diagnostics` drains `quarto.warn` / `quarto.error` records. 6 tests.
- **`LmdExecuteStage`** (`stage/stages/lmd_execute.rs`): a pre-order walk numbers the marked cells, an async loop runs them (partitioning `--|` options first), and a post-order splice replaces each cell by its output list. Skips building a Lua state when the document has no cells. 7 tests, run through `execute_lmd_cells` on parsed documents with no stage context.
- **`LmdDesugarStage`** now marks instead of merely displaying.
- Stage lists: 29 stages on the HTML list; `lmd-execute` also survives into the pandoc leg.

### What the output looks like

The motivating example in immediate mode,

````qmd
::: foo

```{lmd}
local out = pandoc.Blocks({})
for _, v in ipairs({ "a", "b", "c", "d" }) do
  out:extend({=: **`{lmd} v`** :=})
end
return out
```

:::
````

renders to a div holding four paragraphs, `**a**` through `**d**`. A cell that sets `greeting = "Hello"` followed by

````qmd
```{lmd}
--| echo: true
return {=:
### `{lmd} greeting`, world

A paragraph with *emphasis*.
:=}
```
````

renders to the echoed Lua, then `### Hello, world {#lmd-greeting-world}` and the paragraph. A cell with no `return` leaves nothing behind. The two failure cases print, respectively,

```
Error: lmd cell did not return blocks
  … an lmd cell must return a Block, a list of Blocks, or nil; this one returned Inlines
  ℹ Return a Block, a list of Blocks, or nil. Write `{=: … :=}` for a markdown fragment that is blocks.
Error: lmd cell failed
  … runtime error: [string "lmd cell 6"]:1: boom
```

each underlining the cell in the source, and leave the desugared cell displayed.

### Observations

1. **String splices already work, by accident of pampa.** `greeting` is a Lua string spliced into a `Header` content list; pampa's fuzzy inline coercion turned it into `Str("Hello")`, and a string with spaces would become `Str`/`Space` runs. So observation 2 of spike 1 (no coercion contract for inline splices) is partly answered by the runtime: strings and `Inline`s work in inline lists. What is still undefined is a *list* value or a block in an inline list, and anything in block lists other than blocks.
2. **The derived heading id is now visible in output**: `{#lmd-greeting-world}` is the reader's auto id for the placeholder text. Spike 1's observation 3 stands and now has a user-visible effect.
3. **Lua error messages name the cell by number** (`lmd cell 6`), which is the cell's position among lmd cells, not a line in the document. The diagnostic's location underlines the right cell, so this is cosmetic, but a `file:line` chunk name would read better.
4. **`pandoc.Nodes` fails at run time**, as designed: an unmarked fragment in a cell that runs produces "pandoc.Nodes is not implemented yet" with the hint to use a marked form.

## Spike 3 (done 2026-10-12): inline cells in paragraphs

`` `{lmd} expr` `` anywhere inline (paragraphs, headings, list items, emphasis, captions) is now an inline cell: the desugar stage strips the tag, desugars the expression (so it may contain `{=[ … ]=}` fragments) and marks the `Code` node; the execute stage evaluates it in the same Lua state as the block cells and splices the inlines in.

### Contract

The mirror image of the block cell contract: the expression may evaluate to an `Inline`, a list of `Inline`s (a `pandoc.Inlines` or a plain table that may also hold strings), a string, or `nil` (the inline vanishes). A string is split the way pandoc's own coercion does it, into `Str` and `Space` runs. A `Block`, `Blocks`, a number, or a function is an error naming the value, with a hint pointing at `{=[ … ]=}`.

This settles spike 1's observation 2 for inline positions, and it is the same rule pampa's constructors already apply to spliced values inside fragments, so `pandoc.Strong(name)` and an inline-fragment markdown that holds `` `{lmd} name` `` agree.

### Ordering

Block and inline cells are numbered by one pre-order walk (`ast_walk::ListVisitor` over `block_children_mut` / `inline_children_mut`), so document order holds across kinds: an inline cell in the paragraph above a block cell runs first and sees `nil` for a global the block cell sets; the one below sees the value. The hand-rolled block-only walk from spike 2 is gone.

### Failures

Same policy as block cells: an error diagnostic located at the code span (the underline lands on the span, not the paragraph), and the span stays in the document as a plain `lmd` code span holding the desugared expression. There is no `echo` for inline cells; there is nowhere to put options.

### Mentions versus cells

This plan was the first casualty: its prose says `` `{lmd}` `` in several places, and a bare tag with no expression became an empty cell that failed to parse. The rule is now that `` `{lmd}` `` alone is a *mention* and is left as code, the same convention the docs rely on when they write `` `{python}` ``. To show the inline-cell syntax itself, write it as code containing backticks, `` `{lmd} expr` `` inside a double-backtick span: the code's text then starts with a backtick, not the tag. A `{lmd}` tag followed by an expression in prose *will* run, exactly as `` `{python} expr` `` does in a Quarto 1 document; there is no inline equivalent of wrapping a fence in a `markdown` block, which is a gap shared with the other engines.

Desugar failures are now located diagnostics (warning at the cell, with the cell left as written); they used to be bare messages, which is why the empty cell above took a search to find.

### What the output looks like

````qmd
```{lmd}
name = "world"
count = 3
```

## Hello, `{lmd} name`

There are `{lmd} tostring(count)` things, and the third is
`{lmd} {=[ *emphasised* ]=}`. A bold one: `{lmd} pandoc.Strong(name)`.
````

renders to `## Hello, world {#hello-lmd-name}` and "There are 3 things, and the third is *emphasised*. A bold one: **world**." The heading id is the reader's, derived from the placeholder text (spike 1, observation 3; still open).

### Verification

`cargo nextest run -p quarto-core -p pampa -E 'test(lmd)'`: 47 tests (18 desugaring, 12 execute-stage, 8 runtime, the rest grammar and stub).

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

Whichever pair is chosen, nested pairs balance in the scanner the way `{=`/`=}` do today, and `Para` versus `Plain` stops being a syntax question: the block form yields whatever the qmd reader produces (a top-level paragraph is `Para`), and an explicit `pandoc.Plain(…)` covers the rest. The inline form should reject markdown that parses to more than one block, since the author asked for inlines.

### Decision (2026-10-12): `{=`, `{=:`, `{=[`

The third row, with one addition: the unmarked `{= … =}` stays, as a third form. Adding `:` or `[` to the opener adds exactly the specificity it names.

| form | expands to | status |
|---|---|---|
| `{= … =}` | `pandoc.Nodes({ blocks })` | `pandoc.Nodes` is a stub in pampa that errors when called |
| `{=: … :=}` | `pandoc.Blocks({ blocks })` | works |
| `{=[ … ]=}` | `pandoc.Inlines({ inlines })` | works; the markdown must parse to one paragraph or nothing |

`pandoc.Nodes` is the interesting one: a value that defers the blocks-versus-inlines decision to the surrounding context, so `pandoc.Para({= text =})` and `pandoc.Div({= text =})` could both work. Its semantics are not designed yet, so the desugaring emits the call and pampa's entry raises an error naming the two marked forms. The emitter hands it the reader's blocks, which carry enough to become either (a lone `Para` unwraps to inlines).

Scanner rules, so they are not re-derived: every `{=` opens a nested fragment whatever follows it, every `=}` closes one when nested, and at depth zero only the current kind's closer (`=}`, `:=}`, `]=}`) ends the content. A bare `=}` at depth zero inside a marked fragment is content. The character after `{=` decides the kind, so `{=[` opens an inline fragment while `{= [link](u)` (with a space) opens an unmarked one.

## Open questions

1. The semantics of `pandoc.Nodes`, the target of the unmarked `{= … =}`: a value that becomes blocks or inlines depending on the constructor it is passed to. The delimiter design notes record the decision that created it.
2. The splice coercion contract for inline and block lmd code inside a fragment (observation 2).
3. The dispatch half: how an `{lmd}` cell's result replaces the cell at its own AST position, and how the desugared Lua becomes a filter that existing Lua filters compose with. Also whether the cell runs at render time only or has a preview-time story.
4. Error locations. `LmdError` carries a position inside the cell text; the warning does not yet map it back to the document.
5. The name.

## Next spikes

- **Spike 4 candidates**, in no particular order: `pandoc.Nodes` semantics (a value that becomes blocks or inlines by the constructor it is passed to); porting `quarto.metadata` from Q1 so the motivating example works as written; dropping reader-derived ids on headings whose text is a placeholder; `file:line` chunk names for Lua error messages.
- Turn the demo documents into integration fixtures with snapshots of the `--to markdown` output.
- The dispatch half proper: whether a cell's Lua should become a filter that other Lua filters compose with, rather than a chunk that runs once at its position.

## Verification (spike 1)

- `cargo xtask ts-test lmd`: 53 of 53 corpus tests.
- `cargo nextest run -p quarto-core -p pampa -E 'test(lmd)'`: 18 desugaring tests, 7 execute-stage tests, 6 runtime tests, plus the `pandoc.Nodes` stub test.
- `cargo test -p xtask tree_sitter`, `cargo clippy -p quarto-core -p tree-sitter-lmd -p xtask --all-targets -- -D warnings`, `cargo xtask lint`: green.
- WASM: not built locally in this session; the crate follows `tree-sitter-doctemplate`\'s build exactly, and `quarto-core` already pulls two other C-backed grammars into the wasm32 tree. CI's build-wasm job is the check.
