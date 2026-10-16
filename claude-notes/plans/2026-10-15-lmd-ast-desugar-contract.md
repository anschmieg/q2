---
title: 'A structural desugaring contract for AST that does not come from the qmd reader (bd-7gqr148v, GH #846)'
date: 2026-10-15
description: 'list-table and definition-list divs built from AST nodes in an lmd cell never become Table / DefinitionList, and a list-table written inside a fragment hits an emitter gap. Diagnosis, the set of markdown desugaring steps every pandoc.read()-like entry point is promised to get, where that set is applied today and where it is missing, and a phased fix.'
status: draft  # decisions D1–D4 settled 2026-10-16; awaiting go-ahead to implement
braid:
  strand: bd-7gqr148v
---

**Strand:** bd-7gqr148v (bug, P2; discovered from bd-lmd-exploration-jqv2nbl4; related bd-76ogq4ew, whose phase 2 lists "fragment gaps (Table, …)"). **Issue:** [quarto-dev/q2#846](https://github.com/quarto-dev/q2/issues/846).

**Branch:** `braid/bd-7gqr148v-lmd-ast-desugar-contract` (topic branch in the main checkout, off `main` @ `e308835bd`; no worktree, by request).

**Precedent:** `claude-notes/plans/2026-02-05-div-transforms-json-input.md` (bd-31lk) extracted `transform_divs` out of the reader's postprocess so pampa's CLI `--from json` gets the div sugar. This plan generalizes that decision into a contract and applies it to the entry points bd-31lk did not know about.

## The problem

An `{lmd}` cell that *builds* a list-table or definition-list from nodes:

```lua
return pandoc.Div({ pandoc.BulletList(rows) }, pandoc.Attr("", {"list-table"}))
```

renders as `<div class="list-table"><ul><li><ul>…`. The same markup written literally in the document body becomes a `<table>`. The workaround of writing the markup literally inside a `{=: … :=}` fragment fails from the other side: the fragment *is* parsed by the full reader, so it becomes a `Table`, and the lmd emitter has no `Table` constructor, so the cell is rejected with "`Table` nodes are not supported inside markdown fragments yet" and left unrun.

The author's goal (porting an Astro/Hugo "ConfigTable" component: a Lua function that turns data into a table) has exactly one working route today: `pandoc.read(markdown_string, "markdown")`, which defeats the point of building nodes.

## Reproduction (confirmed 2026-10-15 on `main` @ `e308835bd` and on the installed `q2 0.33.0`)

The issue's `repro.qmd` plus probes, rendered with `q2 render --to html`:

| # | Input | Expected | Actual |
|---|---|---|---|
| 1 | literal `::: {.list-table}` in the body | `<table>` | `<table>` |
| 2 | `pandoc.Div({BulletList}, {"list-table"})` returned by a cell | `<table>` | `<div class="list-table"><ul>…` |
| 3 | literal `::: {.definition-list}` in the body | `<dl>` | `<dl>` |
| 4 | `pandoc.Div({BulletList}, {"definition-list"})` returned by a cell | `<dl>` | `<div class="definition-list"><ul>…` |
| 5 | `{=: ::: {.list-table} … :=}` fragment | `<table>` | cell not run; warning "`Table` nodes are not supported inside markdown fragments yet" |
| A | `{=: ::: {.definition-list} … :=}` fragment | `<dl>` | `<dl>` (the emitter *does* have `DefinitionList`) |
| B | `pandoc.Para({ pandoc.Image(alt, src) })` returned by a cell | `<figure>`? | `<p><img>` (the reader's implicit-figure sugar is not applied either) |
| C | `pandoc.Div({ Para }, {"list-table"})` (invalid: no rows list) returned by a cell | Q-2-35 warning | silent; div passes through |
| D | `pandoc.read(list_table_markdown, "markdown").blocks` from a cell | `<table>` | `<table>` |
| E | `pandoc.read(json_with_list_table_div, "json").blocks` from a cell | `<table>` | `<div class="list-table">…` |
| F | the same div returned by a user Lua filter (`filters: [f.lua]`) | (see § Out of scope) | `<div class="list-table">…` (same as Quarto 1) |

Probe files: the issue's repro and `probes.qmd` are reproduced as the fixture in Phase 1 below.

## Diagnosis

The qmd reader does two different kinds of work in its postprocess pass (`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`, `postprocess()`), and the code does not distinguish them:

1. **Concrete-syntax normalization.** Fixes to what tree-sitter produced from *this spelling* of the markdown: citation numbering, trailing `LineBreak` → `\` in headers/paragraphs/plains, absorbing a trailing `Inline::Attr` into a header and deriving the header's auto id, `* []` empty bullet items, a table caption parsed as the last row, `CaptionBlock` attachment to the preceding table, `SoftBreak`-after-`LineBreak` cleanup, the `$math$ {attr}` pattern, the citation-suffix pattern, leftover `Inline::Attr` errors, abbreviation coalescing and `merge_strs`, and the Q-2-50 doubled-brace-fence warnings. These only make sense on reader output; several depend on byte positions, and none of the shapes they act on can be built from Lua (there is no `pandoc.Attr` *inline*, no `CaptionBlock` constructor).

2. **Structural desugaring.** Rewrites of *AST shapes* that are legal pandoc AST on their own and that an author can produce either by writing markdown or by constructing nodes:
   - `Div(.list-table)` → `Table` (`transform_list_table_div`), with Q-2-35 for a malformed one;
   - `Div(.definition-list)` → `DefinitionList` (`transform_definition_list_div`);
   - `Para([Image])` with non-empty alt → `Figure` (the `.with_paragraph` handler; pandoc's `implicit_figures`).

   The first two were already split out as `pub fn transform_divs(Pandoc, &mut DiagnosticCollector)` by bd-31lk. The third is still inlined in `postprocess()`.

Layer 2 is applied only where someone remembered to call it:

| AST entry point | Where | Layer 1 | Layer 2 today |
|---|---|---|---|
| document body, includes, engine output, listing templates, front-matter markdown | `pampa::readers::qmd::read` → `postprocess` | yes | yes (all three) |
| `pandoc.read(s, "markdown")` from Lua | `lua/readwrite.rs` → `readers::qmd::read` | yes | yes |
| lmd fragment `{=: … :=}` / `{= … =}` / `{=[ … ]=}` | `lmd/desugar.rs::parse_markdown` → `readers::qmd::read`, then `emit.rs` prints constructors | yes | yes, but the emitter cannot print a `Table` (Figure and DefinitionList it can) |
| **lmd block cell return value** | `stage/stages/lmd_execute.rs::execute_lmd_cells`, `LmdCellValue::Blocks(result)` spliced as-is | n/a | **none** |
| **lmd inline cell return value** (can carry blocks inside a `Note`) | same, `LmdInlineValue::Inlines` | n/a | **none** |
| pampa CLI `--from json` | `pampa/src/main.rs` | n/a | divs only (bd-31lk) |
| document import (docx/odt/…) | `quarto-core/src/import/mod.rs::read_pandoc_json` | n/a | divs only |
| **`pandoc.read(s, "json")` from Lua** | `lua/readwrite.rs` json arm | n/a | **none** (inconsistent with the CLI arm two lines up) |
| user Lua filter return values, shortcode handlers, `quarto.component` | `UserFiltersStage` and friends | n/a | none (Quarto 1 parity: its list-table filter runs in `normalize`, before user filters) |

So the bug is a missing call at two entry points plus one emitter gap, and the reason it was missed is that nothing names the set of steps or says who is owed them.

## The contract

> **Structural desugaring set.** Every block list that enters the render pipeline *other than through the qmd reader* is rewritten by the same three transforms the reader applies, in this order, exactly once:
>
> 1. `Div(.list-table)` → `Table` (invalid shape: Q-2-35 warning, div left as is);
> 2. `Div(.definition-list)` → `DefinitionList` (invalid shape: left as is, no warning, matching the reader);
> 3. `Para([Image])` whose image has non-empty alt text → `Figure` with the alt as caption (pandoc `implicit_figures`).
>
> **Owed to:** lmd block-cell and inline-cell results (per cell, over what the cell returned), lmd fragments (by construction, since they go through the reader; the emitter must be able to print everything the reader produces for the common cases), `pandoc.read(…, "json")`, pampa CLI `--from json`, and document import.
>
> **Not owed to:** values returned by user Lua filters, shortcode handlers and components. These are filter semantics, where pandoc and Quarto 1 both leave constructed nodes alone. A filter that wants the sugar calls `pandoc.read` (markdown or json, both now covered) or builds the canonical node.
>
> **What the reader does that the set does not:** anything in layer 1 above. In particular, adjacent `Str`s are not merged, headers get no auto ids here (the `heading-id` transform does that later for the whole document), and `[^1]` footnote definitions are not resolved (the `footnotes` transform does that later; a cell cannot build them anyway).

The three members are the complete list of shapes in `postprocess()` that (a) can be built from pampa's Lua constructors and (b) change the *kind* of a block. Everything else in postprocess fails (a) or is a text-level fix-up.

### Decisions (reviewed 2026-10-16)

- **D1. Is item 3 (implicit figure) in the set? Yes, applied anywhere a `Paragraph` appears.** The membership rule is "what the reader would have produced from the equivalent markdown", which raised the question of whether the figure transform is uniform like the two div transforms or top-level-only. Checked against pandoc 3.11 (`pandoc -t native`) and q2's reader (`q2 render --to native`, `main` @ `e308835bd`) on 2026-10-16:

  | single-image paragraph with alt text, placed… | pandoc 3.11 | q2 reader |
  |---|---|---|
  | at top level | `Figure` | `Figure` |
  | in a loose bullet / ordered list item (`Para`) | `Figure` | `Figure` |
  | in a tight list item | `Figure` (pandoc's `para` parser converts before the item is compacted to `Plain`) | `Plain [Image]` (tree-sitter yields `Plain` directly; the handler only matches `Paragraph`) |
  | inside a `::: {.wrapper}` div | `Figure` | `Figure` |
  | inside a blockquote | `Figure` | `Figure` |
  | in a pipe-table cell (`Plain`) | `Plain [Image]` | `Plain [Image]` |
  | with empty alt (`![](h.png)`) | `Para [Image]` | `Para [Image]` |
  | with surrounding text | `Para` | `Para` |

  So neither implementation restricts the transform to the top level: pandoc applies it wherever it parses a paragraph, and q2's `.with_paragraph` handler already runs over every `Paragraph` in the tree. The set's rule is the reader's rule: `Paragraph` whose only inline is an `Image` with non-empty alt, at any depth. The tight-list divergence is a reader-representation difference (`Plain` vs `Para`), unrelated to this contract and left as is (noted under § Out of scope). bd-31lk's split of the two div transforms only was not a judgment that the figure transform is scoped differently; it was the narrowest change for pampa's `--from json`.

  Side effect accepted: pandoc JSON carrying `Para [Image]` with alt text becomes a `Figure` on the json entry points. pandoc only emits that shape with `implicit_figures` disabled, so pandoc-produced JSON is unaffected in practice.
- **D2. Where does a Q-2-35 from a cell-built div point? Relocate onto the cell.** A Lua-built node's `source_info` is `SourceInfo::generated(By::filter(chunk, line))` (`lua/types.rs::filter_source_info`); for an lmd cell the chunk is the document path with line padding (`ChunkIdentity`), so the line *is* a document line. The lmd stage re-locates any diagnostic whose location is `generated` onto the cell's span, the way `failure_diagnostic` already does for runtime errors, and adds a detail "built by this lmd cell". Precise-line relocation through `ChunkIdentity::locate` is a nice-to-have; the whole-cell underline is the honest resolution available without a column map (same call spike 7 made). **Deliverable:** Phase 3 ends with a rendered example of the diagnostic in this plan for review of the overall look.
- **D3. Fragment list-tables: emit `Table`.** The alternative (parse fragments without the div sugar and let the per-cell pass do it) would make one code path serve both forms, but it needs a reader knob to skip `transform_divs`, loses the reader's precise Q-2-35 location, and leaves *pipe tables* in fragments unsupported, which is a separate real gap (bd-76ogq4ew phase 2). Printing a `Table` is mechanical: pampa already has `pandoc.Table(caption, colspecs, head, bodies, foot, attr)`, `pandoc.Caption`, `pandoc.Row`, `pandoc.Cell`, `pandoc.TableHead/Body/Foot`, and colspecs are `{ {align, width}, … }` tables (`constructors.rs::parse_colspecs`).
- **D4. User filters do not get the pass.** Regular Lua filters have always been expected to return real `pandoc.Table` and friends, in pandoc and in Quarto 1 (whose list-table filter runs in `normalize`, before user filters). A filter that uses lmd internally gets the desugaring through its lmd blocks. The boundary is documented, not changed.

## Design

### pampa: name the set

`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs` (or a new sibling `desugar.rs` re-exported from the same place, so the bd-31lk callers keep compiling):

```rust
/// The structural desugaring set (see dev-docs/syntax-notes.md § Structural
/// desugaring contract): list-table, definition-list, implicit figure.
pub fn desugar_filter<'a>(collector: &'a RefCell<&mut DiagnosticCollector>) -> Filter<'a>;
pub fn desugar_blocks(blocks: Blocks, collector: &mut DiagnosticCollector) -> Blocks;   // topdown_traverse_blocks
pub fn desugar_inlines(inlines: Inlines, collector: &mut DiagnosticCollector) -> Inlines; // descends into Note content
pub fn transform_divs(doc: Pandoc, collector: &mut DiagnosticCollector) -> Pandoc;       // kept; now `desugar_blocks` over doc.blocks
```

`postprocess()` keeps its own inline `.with_div` and `.with_paragraph` handlers (they are fused with the layer-1 work in the same traversal and carry source-info bookkeeping the standalone pass does not need), but the figure-building code moves into a shared helper so the two sites cannot drift. The transform registry comment at the top of the file gains a "which entry points get this" column and the layer-1/layer-2 distinction.

Idempotence: after a pass there are no valid `.list-table` / `.definition-list` divs and no single-image paragraphs with alt text left, so running the set over already-desugared blocks is a no-op except that an *invalid* list-table div warns again. That is why the pass runs per returned value, never over the whole document.

### quarto-core: apply it at the lmd entry points

`stage/stages/lmd_execute.rs::execute_lmd_cells`:

- block cell: `LmdCellValue::Blocks(result)` → `desugar_blocks(result, &mut collector)` before `out.extend`;
- inline cell: `LmdInlineValue::Inlines(result)` → `desugar_inlines(result, &mut collector)`;
- the collector's diagnostics are relocated per D2 and pushed to `diagnostics`.

A block cell nested inside a fragment is an IIFE inside the parent's Lua, so its value is part of the parent's result and is covered by the parent's pass. `echo` output is a `CodeBlock`, unaffected.

### quarto-core: the emitter prints `Table`

`lmd/emit.rs`: a `Block::Table` arm emitting

```lua
pandoc.Table(
  { long = { … }, short = { … } },        -- or pandoc.Caption(long, short); either is accepted by parse_caption
  { { "AlignDefault", 0 }, … },           -- colspecs; width 0 = ColWidthDefault
  pandoc.TableHead({ pandoc.Row({ pandoc.Cell({ blocks }, "AlignDefault", 1, 1, attr) }) }, attr),
  { pandoc.TableBody({ rows }, { head_rows }, 0, attr) },
  pandoc.TableFoot({ rows }, attr),
  pandoc.Attr(…))
```

using the existing `call`/`list`/`attr_arg` helpers, in both `Pretty` and `LinePreserving` modes. `LmdError::Unsupported { kind: "Table" }` goes away; the unit test that uses a pipe table as its unsupported example switches to a footnote definition (`NoteDefinitionPara`), which stays unsupported, and the `errors.qmd` fixture does the same.

### pampa: `pandoc.read(…, "json")`

`lua/readwrite.rs` json arm: `transform_divs` (now the full set) after `read_completing_source_info`, matching `main.rs` and `import/mod.rs`. The CLI and import call sites switch to the renamed function so all three read the same.

### Documentation

- `dev-docs/syntax-notes.md`: new section "Structural desugaring contract" with the set, the entry-point table and the "not owed to" boundary. This is the artifact the user asked for; the code comments point at it.
- `docs/guides/authoring/lua-filters.qmd`: a short note under the Quarto-specific-nodes material: the list-table / definition-list / implicit-figure sugar is applied to what lmd cells return and to `pandoc.read`, not to what filter functions return.
- `docs/errors/markdown/Q-2-35.qmd`: mention that the warning can also come from a div an lmd cell built, located at the cell.
- The lmd plan's open-question list (`2026-10-12-lmd-exploration.md`, observation 4 / "Table nodes") and bd-76ogq4ew phase 2 get a pointer to this strand.

## Work items (TDD: tests first in every phase)

### Phase 1: pin the current behavior (done 2026-10-16)

- [x] `crates/quarto/tests/integration/fixtures/lmd/desugar.qmd`: the issue's four sections plus the probes (fragment list-table, fragment pipe table, fragment definition-list, node-built figure with `pixel.png`, invalid node-built list-table, `pandoc.read` markdown and json, an inline cell whose `Note` holds a list-table, a block cell nested in a fragment). Snapshotted as `desugar.main.html` + `desugar.stderr`; the first snapshot documents the bug.
  - **Finding:** the fixture had to be pinned through the native HTML pipeline, not `--to markdown` like the other lmd fixtures. The markdown leg hands the AST to pandoc with Quarto 1's Lua filters, whose own `astpipeline-process-list-tables` converts the cell-built divs and hides the bug; it also emits a `Q-11-1` warning carrying an unnormalized temp path. `lmd_snapshots.rs` gained `HTML_FIXTURES` / `render_html` / `render_to`, and `render_to` copies non-`.qmd` files from the fixtures directory so images resolve.
- [x] Unit-level failing tests are written at the start of the phase that fixes them (Phases 2–4 below) rather than all in Phase 1, so that each phase boundary commit is green: the plan's TDD rule (tests before implementation) and the commit rule (full suite green at a boundary) both hold.

### Phase 2: the set, named (pampa) (done 2026-10-16)

- [x] Tests first: `test_json_div_transforms.rs` gained the implicit-figure case and a list-table-inside-blockquote case; `readwrite.rs` gained `pandoc_read_json_applies_the_structural_desugaring_set` (list-table, definition-list and image paragraph round-tripped through JSON come back as `Table`, `DefinitionList`, `Figure`); `pandoc/desugar.rs` has eight unit tests of its own (each member, invalid list-table warns Q-2-35, any depth, through a `Note`, idempotence).
- [x] New module `crates/pampa/src/pandoc/desugar.rs`: `desugar_blocks`, `desugar_inlines`, `desugar_document`, plus the shared per-node rewrites `desugar_div` and `implicit_figure` (`pub(crate)`). the `.with_div` of `postprocess()` and the figure branch of `.with_paragraph` now call those helpers, so the reader and the set cannot drift. `transform_divs` is gone (its three callers moved); the list-table / definition-list validators and transforms stay in `postprocess.rs` as `pub(crate)`.
  - Implementation note: the figure rewrite returns `FilterResult(…, true)` and the div rewrites `FilterResult(…, false)`, exactly as the reader did, so nested sugar inside a list-table cell is (still) not rewritten, matching the reader.
- [x] Registry comment at the top of `postprocess.rs` names the set, the shared helpers and the entry points.
- [x] `main.rs` and `import/mod.rs` call `desugar_document`; the `readwrite.rs` json arm calls it too (fixture section 11 passes). `pandoc.read` has no diagnostics channel (the Lua `quarto._diagnostics` table carries kind, message and location only), so a malformed list-table's Q-2-35 is dropped on that arm, as the CLI arm always has; cell-built divs are reported in Phase 3 where the stage owns the diagnostics.
- [x] Full `pampa` (5045) and `quarto-core` (6093) suites green; clippy clean.

### Phase 3: lmd cell results (quarto-core) (done 2026-10-16)

- [x] Tests first, in `lmd_execute.rs`: the six tests listed in the plan, all green.
- [x] `execute_lmd_cells` runs the set per cell result (`desugar_cell_blocks` for block cells, `desugar_cell_inlines` for inline cells) and relocates every diagnostic the pass raises onto the cell (`relocate_to_cell`). Fixture sections 2, 4, 8, 9, 12 and 13 now render as the reader would have: tables, a definition list, a numbered cross-referenced figure, a table inside the footnote, a table from the nested cell; section 9 keeps its div and warns.
  - Relocation is unconditional, not only for `SourceInfo::generated` locations as D2 first said: a node `pandoc.read` returned carries an offset into that call's *private* source context, whose file ids collide with the document's, so no location the pass sees can be trusted against the document. Nodes the reader parsed never reach the pass with sugar left in them, so nothing precise is lost.
  - The cell's `anchor.span` is the Lua text of the cell, not the fenced block, so the underline is the code the author wrote.
- [x] Full `quarto-core` suite (6099) green; clippy clean.

#### Diagnostic example (fixture section 9, `q2 render --to html`, ANSI stripped)

```
Warning: [Q-2-35] Invalid List-Table Structure
    ╭─[ <dir>/desugar.qmd:94:1 ]
    │
 94 │ return pandoc.Div({ pandoc.Para({ pandoc.Str("not a list") }) }, pandoc.Attr("", {"list-table"}))
    │ ────────────────────────────────────────────────┬────────────────────────────────────────────────
    │                                                 ╰────────────────────────────────────────────────── list-table div's last block must be a bullet list (the rows)
────╯
ℹ The node was built by this lmd cell.
ℹ Check the list-table documentation for the correct structure?
```

The title, code, problem and hint are the reader's own Q-2-35; the location and the first info line are what the lmd stage adds.

### Phase 4: tables in fragments (quarto-core emitter) (done 2026-10-16)

- [x] Tests first: `lmd/desugar.rs` gained `table_in_a_fragment_is_emitted` (pipe table with a right-aligned column, and a list-table with `widths` that the reader already turned into a `Table` with percentage colspecs) and `line_preserving_table_stays_on_its_lines` (the whole table on one line, exact text pinned); `lmd_execute.rs` gained `tables_in_fragments_run_and_are_spliced_in` (end to end: both forms come back as `Block::Table`). The two tests that used a pipe table as their *unsupported* example (`unsupported_node_is_an_error_without_a_location_when_unanchored`, `lmd_desugar::an_unsupported_node_is_located_at_the_node`) now use a footnote definition, whose `NoteDefinitionPara` stays unsupported.
- [x] `Block::Table` arm in `emit.rs`: `pandoc.Table(caption, colspecs, pandoc.TableHead(rows, attr), { pandoc.TableBody(body, head, row_head_columns, attr) }, pandoc.TableFoot(rows, attr), attr)`, rows as `pandoc.Row({ pandoc.Cell(blocks, align, rowspan, colspan, attr) }, attr)`, colspecs as `{ {"AlignX", pandoc.ColWidthDefault | number} }`. The caption printer is shared with `Figure`. Works in both modes through the existing `call` / `list` helpers. `Unsupported("Table")` is gone.
- [x] the unsupported example in `errors.qmd` is a footnote now; `errors` and `desugar` snapshots updated (fixture sections 5 and 6 render as tables; the desugar fixture's stderr is down to the one intended Q-2-35).
- [x] Full `quarto-core` suite (6102) green; clippy clean on `quarto-core` and `quarto`.

### Phase 5: docs and bookkeeping

- [x] `dev-docs/syntax-notes.md` § Structural desugaring contract (the two layers, the set, the entry-point table, the "not owed to" boundary, how to add a member); `docs/guides/authoring/lua-filters.qmd` § "Tables, definition lists and figures built from Lua"; `docs/errors/markdown/Q-2-35.qmd` notes the cell-built case; the lmd exploration plan's unsupported-node list no longer names `Table`; bd-76ogq4ew carries a comment that its phase-2 `Table` gap is closed (its plan file lives on that strand's branch, not this one).
- [x] `cargo xtask verify` green (2026-10-16): lint + clippy, fmt, Rust build, tree-sitter grammars, Rust tests, ts-packages, hub-client build (WASM) and tests, trace-viewer, q2-preview-spa. Two environment detours worth knowing: the claude-notes render in the lint step rejects an apostrophe right after a code span (writing-notes rule 1), which this plan tripped twice; and the main checkout's `node_modules` predated the 2026-10-14 `@modelcontextprotocol/server` bump, so `npm install` had to run before ts-packages built.
- [x] PR [#850](https://github.com/quarto-dev/q2/pull/850) open against `main` (2026-10-16); strand closes with the merge.

## Out of scope (recorded so they are not re-discovered)

- **User-filter return values** (probe F): Quarto 1 parity; see D4.
- **Images in tight list items**: pandoc makes a `Figure`, q2's reader makes `Plain [Image]` (D1 table). A reader-level divergence with its own consequences (crossref, `fig-` ids in tight lists); not part of this contract.
- **Footnotes in fragments** (`NoteReference` / `NoteDefinitionPara` unsupported by the emitter): bd-76ogq4ew phase 2.
- **Column-exact relocation of Q-2-35 inside a cell**: waits for `CodeBlock.text_source` (bd-f6tt2exg).
- **Numbers in inline splices, string-typed grammar positions**: bd-0o82ly9c.
- **Lua-callable `quarto.desugar(blocks)` for filter authors**: would be a one-line binding over `desugar_blocks` if D4 ever flips; not needed for #846.

## References

- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`: `postprocess()`, `transform_divs()`, `validate_list_table_div()`, `is_valid_definition_list_div()`, the `.with_paragraph` figure handler.
- `crates/quarto-core/src/stage/stages/lmd_execute.rs::execute_lmd_cells`; `crates/pampa/src/lua/lmd.rs::classify_cell_value` (the cell contract the pass runs after).
- `crates/quarto-core/src/lmd/desugar.rs::parse_markdown`, `crates/quarto-core/src/lmd/emit.rs::block` (the `Unsupported("Table")` arm).
- `crates/pampa/src/lua/readwrite.rs::pandoc_read` (qmd vs json arms); `crates/pampa/src/main.rs` `"json"` arm; `crates/quarto-core/src/import/mod.rs::read_pandoc_json`.
- `crates/pampa/src/lua/constructors.rs`: `pandoc.Table`, `parse_colspecs`, `pandoc.Caption`, `Row`, `Cell`, `TableHead/Body/Foot`.
- Quarto 1: `external-sources/quarto-cli/src/resources/filters/normalize/astpipeline.lua` (`astpipeline-process-list-tables` runs in normalize, before user filters).
- Prior plans: `2026-02-05-div-transforms-json-input.md` (bd-31lk), `2026-10-12-lmd-exploration.md` (spikes 1–7), `2026-10-15-lmd-modules-and-qol.md` (bd-76ogq4ew phase 2 fragment gaps).
