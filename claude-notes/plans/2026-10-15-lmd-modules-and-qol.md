---
title: 'lmd infrastructure: `require` of `.lmd` modules, component call sites, and an ecosystem survey (bd-76ogq4ew)'
date: 2026-10-15
description: 'Survey of the JSX/MDX, Hugo/templating, Typst and Lua-dialect ecosystems for quality-of-life features worth borrowing into lmd, a design for loading `.lmd` modules through `require`, and a phased plan for the reusability work that follows the landed lmd feature.'
status: in-progress  # survey done 2026-10-15; decisions recorded and go-ahead given 2026-10-16; Phase 1 implemented, merged with origin/main (post #855) and `cargo xtask verify` green (2026-10-16); Phase 1 PR open, Phases 2-3 to follow
braid:
  strand: bd-76ogq4ew
  priority: P2
  labels: [lua, lmd, exploration]
---

**Strand:** bd-76ogq4ew (discovered from bd-lmd-exploration-jqv2nbl4; related bd-2oqen84t, bd-o7ujtnk3, bd-f6tt2exg, bd-lmd-docs-41adv2nx, bd-7gqr148v). **Branch:** `braid/bd-76ogq4ew-lmd-reusable-modules-require` (topic branch in the main checkout, off `main` @ `5dbf0bc3d`; go-ahead given 2026-10-16). PR #850 (the structural desugaring contract, bd-7gqr148v) merged to `main` as `5dbf0bc3d` on 2026-10-16. PR #855 (the guide, `feature/bd-lmd-docs-41adv2nx-lmd-docs`) is still open and touches the files Phase 1 edits (`lmd_execute.rs`, the `errors.qmd` fixture and its snapshots). Implementation starts from `main` once #855 has merged; if Phase 1 starts earlier, it branches off #855 after that branch has merged `main`, since it needs the guide page. **Prior plans:** `2026-10-12-lmd-exploration.md` (the language and the stages), `2026-10-13-lmd-listings.md` (`meta.listings`), `2026-10-13-react-components.md` (`quarto.component`), `2026-10-15-lmd-docs.md` (the guide, PR #855), `2026-10-15-lmd-ast-desugar-contract.md` (the structural desugaring contract and `Table` in fragments, PR #850).

## The question

lmd landed in PRs #823, #826 and #828: a `{lmd}` cell is Lua with markdown fragments as expressions, cells share one Lua state per document, and the document metadata and resolved listings are in scope. The design was aimed at the neighbourhood of JSX (markdown in place of XML) and of Hugo\'s programmatic shortcodes (loops and conditionals in the page). What it does not yet have is a story for **reuse**: a function defined in one cell is visible to the later cells of the same document and nowhere else.

The first idea on the table is `require("foo.lmd")`, so a module written in the Lua+markdown dialect can be loaded from a cell. That needs the desugar step to run before `load()`, which is new: today the desugarer runs only over the cells of the document being rendered. The two obvious shapes are a Rust-side loader that desugars on the way in, or a Lua-callable desugar function that a loader written in Lua could use; a third is ahead-of-time compilation of `.lmd` to `.lua`.

Before building any of that, this plan surveys what the JSX, Hugo and adjacent ecosystems learned about the same problems, and proposes a sequence.

## Where lmd stands today

Facts verified in this session against `main` (2d34c48b9) and against PR #855\'s branch (61a0ce99f); PR #850 has since merged to `main` (5dbf0bc3d), so the survey\'s mapping notes rest on something concrete.

**Stages.** `lmd-desugar` is stage 11 of the HTML list, before `engine-execution`, so no engine sees a `{lmd}` cell. `listing-resolve`, then `lmd-execute` (stage 19), then `user-filters-pre`. The same list drives the pandoc leg and `--to markdown`.

**Runtime.** `pampa::lua::lmd::LmdRuntime` builds the same environment a Lua filter gets, through `create_filter_environment`: `pandoc.*`, `quarto.*` (including `quarto.doc.add_html_dependency` and `quarto.doc.include_text`), `FORMAT`, `meta`, `quarto.metadata.get`, and the scoped `require` from `quarto_api.rs`. The document path stands in for the script path, so `require` resolves relative to the document\'s directory.

**`require` today.** The scoped `require` tries `<dir>/<name>.lua`, `<dir>/<name with dots as slashes>.lua` and `<dir>/<name>/init.lua` against the module-dir stack and the script-dir stack, loads the module in a sandboxed environment that inherits globals, caches by resolved path, and falls back to the stock `require` when the target has one (native only; the WASM stdlib has no `package` library). File reads go through `SystemRuntime`, so the same code runs on native and in the hub client\'s virtual filesystem.

Three renders from a scratch directory:

| document | result |
|---|---|
| a cell doing `require("card")` with `card.lua` beside it, returning a table of functions that build `pandoc.Div`s | renders; the module\'s functions work from the cell |
| `require("card_lmd.lmd")` with `card_lmd.lmd` beside it | runtime error at the `require` line: the stock Lua `require` reports the native `package.path` search (`/usr/local/share/lua/5.4/card_lmd/lmd.lua` and friends, plus "can\'t load C modules in safe mode"); the scoped loader never looked for `.lmd` |
| the same module saved as `card_lmd2.lua` | syntax error from Lua at the first `{=:`, located at the `require` line |

So a `.lua` module works, and the two failure modes for the dialect are a noisy fallback message and an un-desugared chunk. (The second also shows the scoped loader\'s chunk name is the absolute path, which Lua abbreviates to `...` past 60 bytes; bd-o7ujtnk3.)

**The include workaround.** `{{{< include _cards.qmd >}}}` of a file holding a function-defining cell, followed by a cell calling the function, renders correctly (include expansion runs before `lmd-desugar`). It is a global-namespace prelude, not a module: it defines globals, has no identity to cache under, re-runs on every inclusion, carries a document\'s shape (front matter is possible but meaningless), and cannot be shared across projects except by copying.

**Where the pieces live.** The desugarer is `quarto_core::lmd` (`desugar.rs` 606 lines, `emit.rs` 544, `anchor.rs` 210); it depends on `pampa::readers::qmd::read` for the fragment markdown, on `quarto_pandoc_types` for the emitter, on `quarto_source_map` for anchors, and on one small `quarto_core::engine` helper (`engine_cell_lang`, about twenty lines). The Lua runtime and the scoped `require` are in pampa. pampa cannot call into quarto-core, which shapes the loader design below.

**Gaps noticed on the way.**

- The user guide is in flight: PR #855 (bd-lmd-docs-41adv2nx) adds `docs/guides/authoring/lmd.qmd`, organised by pattern of use (metadata, repeating over a list, shared state, fragments as functions, conditional content, a filter on a fragment, echo and failures), eight example projects under `examples/lmd/`, a `patterns.qmd` snapshot fixture, and an `lmd` diagnostics subsystem with codes `Q-26-1` to `Q-26-7`. Its "Current limitations" section already says a module written in lmd does not load yet and that reusable modules are coming, so Phase 1 owes that page a modules section. Its smoke tests filed bd-3h6iasmz (`pandoc.OrderedList` default attributes break the pandoc leg), bd-onsqz7sv (a shortcode inside a fragment is rejected with the diagnostic on the front matter) and bd-iqd6niat (inline cells reject numbers). The guide\'s "Writing about lmd" rule settles how to *display* a cell (a `markdown` fence with more backticks; a bare `{lmd}` tag is a mention), which narrows the escaping item below to a literal `{=` inside a fragment.
- `crates/pampa/tests/wasm_lua.rs` does not mention lmd: the runtime has no WASM test, though it is on the WASM pipeline through quarto-core.
- The hub client and `quarto-highlight` know nothing of `lmd`; an echoed cell (`--| echo: true`) is an unhighlighted `lmd` code block, and the editor treats a `{lmd}` cell as a block of unknown language.
- Fragments cannot contain a footnote (the reader\'s `NoteDefinitionPara` is resolved later in the pipeline), a shortcode (bd-onsqz7sv), CriticMarkup or a `Custom` node; each fails the cell with `Q-26-2`. Inline cells in attribute values are not evaluated (bd-0o82ly9c). Tables *are* covered as of PR #850 (bd-7gqr148v, GH #846, merged to `main` 2026-10-16): the emitter prints `pandoc.Table`, so pipe tables and list-tables work inside fragments, and the same PR names a **structural desugaring contract** (`pampa::pandoc::desugar`: list-table div to `Table`, definition-list div to `DefinitionList`, single-image paragraph to `Figure`) that is applied once to every block list entering the pipeline other than through the reader: lmd cell results, fragments, `pandoc.read(…, "json")`, import. It is deliberately *not* applied to what user filters return. Two consequences for this plan: nodes a module builds are covered because they come back through a cell, and any new entry point this plan adds (`quarto.include` returning `Blocks`, `quarto.markdown` if it ever takes JSON) owes the set too; the "Structural desugaring contract" section of `dev-docs/syntax-notes.md` has the table to extend.

## Survey

Three research passes, each over a different neighbourhood, then merged. Organised by feature; each item ends with how it maps onto lmd. Sources are linked inline.

### A. JSX, MDX, Astro, Svelte, Markdoc {#survey-a}

**Markup literals and fragments.** JSX makes markup an expression and `<>…</>` returns several siblings without a wrapper; Astro allows several root elements in a template ([Astro syntax](https://docs.astro.build/en/basics/astro-syntax/)). lmd already has the stronger form: `{=: … :=}` is a `pandoc.Blocks`, and the blocks/inlines/`Nodes` typing is more precise than JSX\'s one node type. Nothing to borrow except the lesson that a fragment should stay the default return shape of a component.

**Components, children and slots.** A JSX component is a function of props whose nested markup arrives as `props.children`. Astro adds a *declarative call site*: `<Card title="x"><p>body</p></Card>` with `<slot />`, named slots (`<slot name="footer" />` filled by a child carrying `slot="footer"`), fallback content inside the slot, and `Astro.slots.has(name)` / `Astro.slots.render(name, args)` for conditional wrappers ([Astro components](https://docs.astro.build/en/basics/astro-components/)). Vue\'s scoped slots pass data from the child up into the slot content, which is how a "renderless" list component hands each item back to the caller ([Vue slots](https://vuejs.org/guide/components/slots.html)). Svelte 5 replaces slots with **snippets**: `{#snippet row(x)}…{/snippet}` are first-class values; declared inside a component\'s tags they become props, the rest becomes an implicit `children` snippet ([Svelte snippet](https://svelte.dev/docs/svelte/snippet)). Jinja\'s `{% call macro() %}…{% endcall %}` with `caller()` is the same idea, two decades earlier.

*Mapping.* This is lmd\'s largest gap, and both the JSX and the Hugo pass arrived at it independently. A component is already a Lua function returning a fragment, but the call site is code-first: wrapping *markdown the author is writing* means nesting it inside a fragment inside a cell, which inverts the author\'s mental model. The Quarto analogue of Astro\'s call site is the fenced div: a div whose attributes name a function, whose body arrives parsed as `Blocks`, and whose child divs with `slot=` fill named slots; a bracketed span is the inline case. Svelte\'s "a snippet is a value" maps directly onto Lua: a slot is a `Blocks` value or a function returning one, in the same props table. See [Component call sites](#component-call-sites).

**Props: destructuring, defaults, spreading, validation.** JSX destructures with defaults and spreads `{...props}`; Astro types `Astro.props` with an interface. Markdoc is the relevant one: tags are declared in a schema with `attributes: { type: { type: String, default: 'note', matches: [...] } }`, allowed `children`, `selfClosing`, plus `validate` and `transform` hooks, and validation is a separate phase from transformation ([Markdoc tags](https://markdoc.dev/docs/tags)). *Mapping.* Lua tables give named props and spreading for free. Worth borrowing is the declarative schema: a `props=` table on a component declaration lets the execute stage emit a located diagnostic ("`level` must be a number") at the call site instead of a Lua `attempt to index nil`, coerces div attributes (which arrive as strings) to the declared types, and gives the hub client completion data. It only pays off once there is a markup-side call site.

**Import, export and distribution.** MDX files are ES modules: `import {Chart} from './chart.jsx'`, `export const meta`, a default export as layout; components come from npm the same way ([What is MDX](https://mdxjs.com/docs/what-is-mdx/)); MDX v3 needs a `baseUrl` so relative imports resolve against the document ([MDX v3 migration](https://mdxjs.com/migrating/v3/)). Markdoc keeps all code out of the document and has `{% partial file="header.md" variables={...} /%}` resolved through `config.partials` ([Markdoc partials](https://markdoc.dev/docs/partials)). *Mapping.* `require()` relative to the script is the ESM analogue and already exists for `.lua`. The missing steps are `.lmd` resolution with desugaring on load, the Lua convention of returning a table of components, a project-level autoload so pages do not repeat the `require`, and distribution through `_extensions/`, which is Quarto\'s npm.

**Element remapping and layouts.** MDX\'s `components` prop maps element names to components (`{ h1: 'h2', img: Figure }`) and the `wrapper` key (or an in-file default export) wraps the whole document; nested providers merge, inner wins ([Using MDX](https://mdxjs.com/docs/using-mdx/)). Markdoc\'s `nodes` config does the same. *Mapping.* This is a pandoc filter with a nicer spelling. A module could register element handlers whose bodies are fragments, run as a filter right after cell execution and before `user-filters-pre`. Note the layering lesson: the JS world separates *components* (call sites) from *plugins* (remark/rehype over the tree); lmd collapses both into Lua, which is a simplification, but it means the stage order (lmd before user filters) must be documented as the equivalent of remark-before-rehype.

**Expressions, interleaving and escaping.** MDX allows `{expr}` anywhere and decides block versus inline from line layout (text and tags on one line are inline); `<` and `{` need `\<` and `\{` ([What is MDX](https://mdxjs.com/docs/what-is-mdx/)). Astro\'s `is:raw` turns off template parsing for an element\'s children ([directives](https://docs.astro.build/en/reference/directives-reference/)). *Mapping.* lmd made the block/inline decision explicit in the delimiter rather than inferring it from layout, which is the better decision and should be kept. Two things to borrow: a documented escape for a literal `{=` or `=}` inside a fragment, and an `is:raw`-like fragment that is not scanned for nested `{lmd}` code (needed to *document* lmd in lmd).

**Conditional rendering and iteration.** React uses `cond && <X/>` and `items.map(...)`; Solid replaces these with `<Show>`, `<Switch>`, `<For>` ([Solid control flow](https://docs.solidjs.com/concepts/control-flow/conditional-rendering)); Markdoc has `{% if %}` with only `undefined`, `null`, `false` falsy. *Mapping.* Lua `if` and `for` around fragments already read better than either. The one small sugar is for the author who is *inside* a fragment: `lmd.when(cond, blocks)` and `lmd.map(xs, fn)` flattening to `Blocks`. React\'s `key` is for reconciliation and has no analogue.

**Raw escape hatches.** React `dangerouslySetInnerHTML`, Astro `set:html` (unescaped), `set:text` (escaped), `<Fragment set:html>` ([directives](https://docs.astro.build/en/reference/directives-reference/)). *Mapping.* `pandoc.RawBlock("html", s)` exists and is format-aware, which is better. The lesson is naming: give raw output a visible, deliberate spelling and keep it out of the default fragment path, so a fragment never silently becomes raw HTML.

**Scoped styles and per-component assets.** Svelte, Vue and Astro single-file components carry a `<style>` scoped to the component, deduplicated by the bundler. *Mapping.* A module can call `quarto.doc.add_html_dependency` once at `require` time; dedupe by module path. Cheap, and it composes with `quarto.component`.

**Async data at render time.** MDX v3 allows `await` in expressions; Astro fetches in frontmatter at build time ([MDX v3](https://mdxjs.com/blog/v3/)). *Mapping.* Cells run at render time with `meta.listings.*` in scope; the shared model is "component runs at build, output is static". Nothing to borrow.

### B. Hugo and its peers (Eleventy, Jekyll/Liquid, Jinja/Nunjucks, pandoc templates) {#survey-b}

Baseline: Quarto\'s shortcode handlers are `function(args, kwargs, meta, raw_args, context)` with no paired form (`crates/pampa/src/lua/shortcode.rs`).

**Paired shortcodes and how inner content arrives.** Hugo `{{{< note >}}}…{{{< /note >}}}` gives the template `.Inner` as a raw string, and the call notation decides *when* the shortcode runs relative to the markdown renderer: `{{% %}}` before Goldmark (inner and output are markdown; headings count for the TOC), `{{{< >}}}` after (output is HTML; inner markdown needs `.Inner | .Page.RenderString`). Nested shortcodes render inside out; the child sees `.Parent` and `.Ordinal` ([shortcode templates](https://gohugo.io/templates/shortcode/)). `.InnerDeindent` exists only because a shortcode indented in a list item becomes a CommonMark indented code block ([InnerDeindent](https://gohugo.io/methods/shortcode/innerdeindent/)). Eleventy\'s `addPairedShortcode(name, (content, ...args) => …)` gets the inner content as the first argument, already run through the template engine, and the result goes through markdown-it ([11ty shortcodes](https://www.11ty.dev/docs/shortcodes/)). Jinja\'s `{% call %}` passes the body as a callable `caller()` that the macro may call with arguments, so "wrapper that iterates and hands each item back to the body" is a one-liner ([Jinja templates](https://jinja.palletsprojects.com/en/stable/templates/)).

*Pain points, measured by forum volume.* The `%` versus `<` split is the most-discussed Hugo problem: tables break under `%` because output is re-parsed as markdown, HTML shortcodes nested inside `%` shortcodes get mangled, a template cannot tell which delimiter called it, and the 0.55 semantics change produced a wave of "markdown not rendering in `.Inner`, add `markdownify`" threads ([delimiters in tables](https://discourse.gohugo.io/t/shortcode-inside-markdown-table-why-does-delimiters-matter/21387), [detect delimiter](https://discourse.gohugo.io/t/any-way-to-determine-if-a-shortcode-delimiter-is-or/47182), [0.55](https://discourse.gohugo.io/t/misunderstanding-of-shortcode-syntax-in-0-55/18538?page=2)). Indentation-as-code-block bites Hugo and Eleventy both; Eleventy also documents that wrapping paired content in a `<div>` stops the markdown inside from rendering ([11ty markdown](https://www.11ty.dev/docs/languages/markdown/)).

*Mapping.* Every one of those bugs is a string-pipeline bug: inner content is text, re-parsed at a time the author does not control. lmd\'s fragments are AST already, so a wrapping construct must receive *parsed* `Blocks`, never a string, and the `%`/`<` dichotomy, `markdownify`, `RenderString` and `InnerDeindent` never arise. Jinja\'s `caller(item)` maps onto passing the body as a Lua function rather than a value when the component wants to call it per item.

**Arguments.** Hugo: `.Get "name"` or `.Get 0`, `.Params` is a map or a slice, one call may not mix styles, and `errorf`/`warnf` with `$.Name` and `$.Position` point at the call site ([shortcodes](https://gohugo.io/content-management/shortcodes/)). Jekyll: `{% include f.html key="v" %}` with the famous limitation that values cannot contain `{{ }}`, forcing `{% capture %}` first ([Jekyll includes](https://jekyllrb.com/docs/includes/)). Jinja macros have defaults, `varargs` and `kwargs`. *Mapping.* Lua has all of it. Borrow (a) call-site positions in diagnostics, which lmd\'s line mapping already provides for cells and should keep for any component-call form, and (b) Jekyll\'s lesson: argument values must be able to be fragments, which falls out of a component being a Lua call.

**Partials, return values, caching.** Hugo `partial "name" CONTEXT` passes an explicit context (forgetting the dot is the classic error), may `return` any value, and `partialCached "name" . VARIANT…` memoises per site with a key the partial cannot see; wrong variants silently reuse stale output ([partial](https://gohugo.io/functions/partials/include/), [partialCached](https://gohugo.io/functions/partials/includecached/)). Jinja `import` is cached by default and imported macros cannot see the caller\'s context unless `with context`, which disables caching. *Mapping.* A partial with `return` is a Lua function returning a fragment; lmd has the good version. The cross-page cache question arises only if a project-level Lua state appears (site nav, cards shared across pages); then offer an explicit `memoize(fn, key)` with a visible key rather than an implicit `partialCached`.

**Packaging, discovery and import of libraries.** Hugo theme components are a left-to-right override list; `layouts` and `static` merge by file, `data` and `i18n` deep-merge; Hugo Modules make any Go module a component with `module.mounts` mapping source directories onto `layouts`, `assets`, `content`, plus `replacements` for local development and vendoring ([theme components](https://gohugo.io/hugo-modules/theme-components/), [module config](https://gohugo.io/hugo-modules/configuration/)). Shortcodes, partials and hooks are found by *file path* in reserved directories (`layouts/_shortcodes/media/audio.html` is called as `media/audio`) with output-format and language suffixes ([new template system](https://gohugo.io/templates/new-templatesystem-overview/)). Jinja: `{% import "forms.html" as forms %}`, a file of macros is a namespace. Pandoc: `$partial()$` resolved next to the main template, then the user templates directory ([pandoc partials](https://pandoc.org/MANUAL.html#partials)). *Pain points:* theme shortcodes silently shadow user shortcodes of the same name, and the lookup rules are complex enough that users ask for "print the chosen template" tooling ([lookup debugging](https://discourse.gohugo.io/t/how-to-view-template-lookup-order-for-a-given-page/34744)). *Mapping.* The Jinja model fits: `local ui = require("components/cards")` gives an explicit namespace, no shadowing, no lookup table to learn. Distribution rides `_extensions/`, so shortcodes, filters and lmd modules share one packaging story. Avoid Hugo\'s implicit by-filename global namespace.

**Render hooks.** Hugo overrides how a markdown element renders with `layouts/_markup/render-{link,image,heading,codeblock,blockquote,table,passthrough}.html`, including `render-codeblock-LANG.html` per language; the canonical example is a mermaid hook that emits `<pre class="mermaid">` and sets a page store flag so the base template loads the script once ([render hooks](https://gohugo.io/render-hooks/introduction/), [code blocks](https://gohugo.io/render-hooks/code-blocks/)). *Mapping.* Quarto has the general mechanism (a Lua filter\'s `Link = function(el) … end`); a hook is a filter with a declarative on-ramp. What lmd adds is letting the hook body be a fragment, and a per-language code-block hook keyed by class, which the filter API makes awkward today. The "flag and inject once" idiom is `quarto.doc.include_text` and `add_html_dependency`, which exist.

**Per-page scratch state.** Hugo `.Store` (per page, shortcode, site, global) replaced `.Scratch`, with the documented caveat that values set during content rendering are indeterminate until `.Content` is forced ([Store](https://gohugo.io/methods/page/store/)). *Mapping.* Lua upvalues and globals in the shared state give this with none of the ordering hazard, since cells run in document order. Nothing to borrow unless a project-level state appears.

**Data pipelines.** `where`, `range`, `with`, `dict`, `slice`; content adapters (`_content.gotmpl`) generate pages from data with documented sharp edges ([content adapters](https://gohugo.io/content-management/content-adapters/)). *Mapping.* Lua tables and `pandoc.List:filter`, `map`, `sort` cover it; `meta.listings.<id>.items` already has `:map`. The small gap is a `where`-style predicate and sort-by-key library for listing items so authors do not hand-roll comparators (bd-u1rxccq7 is the date-formatting corner of this). Content adapters are a project-phase feature, out of scope for cell execution.

**Markdown-rendering escape hatch.** `markdownify` (strips a lone `<p>`, honours render hooks, loses page context), `.RenderString` (with `display: inline|block`), `.RenderShortcodes` (expand another page\'s shortcodes but keep its markdown, so footnotes and the TOC share one context) ([markdownify](https://gohugo.io/functions/transform/markdownify/), [RenderString](https://gohugo.io/methods/page/renderstring/), [RenderShortcodes](https://gohugo.io/methods/page/rendershortcodes/)). *Mapping.* Fragments make the static case moot. The dynamic case, a markdown *string* from metadata or a data file, is `pandoc.read(str)` plus the inline/block coercion `pandoc.Nodes` already does; expose it as one helper returning a `Nodes`. The `RenderShortcodes` lesson is an invariant lmd already keeps: includes expand before cells run, so one footnote and TOC context.

**Template inheritance.** `baseof` with `block`/`define`; Jinja `extends`/`super()`. *Mapping.* Page layout belongs to Quarto\'s HTML template and doctemplates, not lmd; within a document, slots are named fragment arguments. Not missed.

### C. How Lua dialects hook `require`

Stock Lua 5.4: `require` consults `package.loaded`, then walks `package.searchers` (preload, the Lua searcher over `package.path`, two C searchers); a searcher returns a loader and its data, and the result is cached in `package.loaded` ([manual §6.3](https://www.lua.org/manual/5.4/manual.html#6.3)). Two hook points follow: insert a searcher (discovery by extension) or fill `package.preload` (bundling). LuaRocks packages already-Lua files by module name; dialects ship either compiled `.lua` or the compiler as a dependency.

| dialect | hook | error mapping | AOT |
|---|---|---|---|
| MoonScript | rewrites `package.path` into `package.moonpath` and inserts its loader at position 2, so `.moon` shadows `.lua`; compiles in memory ([API](https://moonscript.org/reference/api.html)) | `to_lua` returns a line table keyed by chunk name; `moonscript.errors.rewrite_traceback` prints `file.moon:N` | `moonc` to `.lua`; `-p` prints |
| Fennel | `fennel.install()` adds `fennel.searcher` over a separate `fennel.path`; `fennel.make-searcher(opts)` for other options ([API](https://fennel-lang.org/api)) | `correlate` emits Lua whose lines match the input where possible; `fennel.traceback` uses a compiler-kept source map keyed by chunk name ([reference](https://fennel-lang.org/reference)) | `fennel --compile`; `--require-as-include` embeds string-literal `require`s into `package.preload` for a single-file build |
| Yuescript | `yue.insert_loader(pos)`, default position 3 ([library](https://yuescript.org/doc/advanced/the-yuescript-library/)) | `reserve_line_number` on by default; `yue.traceback` rewrites what remains | `yue` compiles directories |
| Teal | `tl.loader()` adds on-the-fly compilation of `.tl` ([repo](https://github.com/teal-language/tl)) | annotations are erased in place, so lines are identical and no map is needed | `tl gen` |

*Mapping.* Every dialect converges on a searcher keyed by extension, an in-memory compile under a chunk name of the form `@file.ext` so Lua prints `file.ext:line:`, either line-preserving emission or a chunk-name-keyed source map with a `traceback` override, and an optional AOT path producing plain `.lua` for distribution. lmd differs in one way that decides the design: its compiler is in Rust, and the WASM stdlib has no `package` library at all, so a Lua-side `package.searchers` entry is not available there. The searcher has to be the Rust-side scoped `require` that already exists. Teal is the strongest argument that line-preserving emission is the simplest viable error strategy when the transform is local, which lmd\'s is; MoonScript\'s line tables are the fallback if a fragment expansion ever cannot be padded onto its source lines.

### D. Typst, Pollen, Scribble, MyST, Org, Markdoc, Lua templating

**Typst: content blocks as trailing arguments.** `#` enters code, `[...]` is a content block yielding a `content` value, and "an arbitrary number of content blocks can be passed as trailing arguments to functions": `list[A][B]` is `list([A],[B])`, and an empty argument list may be omitted ([scripting](https://typst.app/docs/reference/scripting/), [function](https://typst.app/docs/reference/foundations/function/)). *Mapping.* lmd\'s fragment is the content block; the missing sugar is the **call form**: Lua already allows `f{...}` and `f"..."` as calls with one table or string argument, so `callout{=: body :=}` and `callout{title="x"}{=: body :=}` are a small grammar extension (`functioncall := prefix markdown_fragment`) that makes "wrap this prose" one token. This is the code-side twin of the markup-side call site in [Component call sites](#component-call-sites).

**Typst: functions, named arguments, sinks, `.with`.** `#let alert(body, fill: red)`, `..args` collecting into an `arguments` value with `.pos()` and `.named()`, spreading, and `f.with(...)` partial application ([arguments](https://typst.app/docs/reference/foundations/arguments/)). *Mapping.* Lua has no named arguments; the idiom is one table argument with the body positional, `callout{title="Note", {=: body :=}}`, which the call form above supports. `.with` is a five-line helper.

**Typst: `content` versus `str`.** "Wherever content is expected, you can also pass a string or none" ([content](https://typst.app/docs/reference/foundations/content/)). *Mapping.* lmd has the split already (strings coerce to `Str`/`Space` runs; `Inlines`, `Blocks`, `Nodes`); the lesson is that every content-accepting API should accept strings, which the inline contract and the constructors do.

**Typst: `import`, `include`, packages.** `#import "bar.typ"` binds a module, `#import "bar.typ": a, b as c`; `#include "bar.typ"` evaluates the file and returns its *content*; packages are `#import "@preview/name:0.1.0"`, always fully versioned, downloaded on demand and cached, described by a `typst.toml` with `name`, `version`, `entrypoint`, `license`, `description`, optional `compiler` and `[template]` ([packages](https://github.com/typst/packages)). *Mapping.* `require` is `import`. The gap is an **`include` equivalent**: a function that evaluates an `.lmd` (or `.qmd`) file with parameters and returns its `Blocks`, distinct from the module value. Versioned, manifest-described packages are the `_extensions/` mechanism; lmd should resolve through it rather than invent a registry.

**Typst: set and show rules, `context`.** `set` scopes a style to the rest of a block or file; `show heading.where(level: 1): it => ...` transforms elements from that point on; `show: rest => ...` wraps everything after; `context` yields a value computed with knowledge of its surroundings and is opaque ([styling](https://typst.app/docs/reference/styling/), [context](https://typst.app/docs/reference/context/)). *Mapping.* A show rule is a pandoc filter declared *inside* the document and scoped to what follows; `quarto.show(Header, fn)` from a cell, applied to the rest of the document, is the lmd analogue and the same mechanism as the element-handler registry in [survey part A](#survey-a). `context` is deferred evaluation, which lmd lacks (cells read earlier cells, not later ones); a late-resolved closure primitive is the analogue, and heavy; revisit when cross-reference needs appear.

**Pollen.** `◊cmd[args]{text}`; every command name is a tag function by default; **`pollen.rkt` is imported automatically by every source file in the same directory and below, with per-subdirectory override**; a `root` tag function receives the whole document, and `decode` hooks post-process outside the authoring layer ([command syntax](https://docs.racket-lang.org/pollen/pollen-command-syntax.html), [decode](https://docs.racket-lang.org/pollen/Decode.html)). *Mapping.* A per-directory implicit prelude (`_lmd.lua`, or whatever the autoload key names) auto-required into every document\'s state is the single biggest boilerplate remover, and it is what the include workaround approximates. Undefined-name-becomes-element conflicts with Lua\'s nil globals; skip.

**Scribble.** `@cmd[datum]{text}` nests; `|{ }|` alternate delimiters let a body contain braces ([reader](https://docs.racket-lang.org/scribble/reader.html)). *Mapping.* Solves "how do I write `:=}` inside a fragment": an alternate or length-varying delimiter such as `{=:| … |:=}`, or Lua\'s own `[==[ ]==]` idea applied to fragments. Same need as MDX\'s `\{` in [survey part A](#survey-a), with a better precedent.

**MyST.** Directives are block extension points (`:::{name} arg`, `:key: value` options, body), roles are inline; plugins export `{ directives: [{ name, arg, options, body, run }], roles: [...], transforms: [{ stage, plugin }] }`; `{eval}` splices a kernel expression inline ([syntax](https://mystmd.org/guide/syntax-overview), [plugins](https://mystmd.org/guide/javascript-plugins)). *Mapping.* Inline `{lmd}` code *is* `{eval}`. The plugin shape is how an `.lmd` module could *register* directives and roles: `return { directives = { callout = { args = ..., run = function(d) ... end } } }`, which is the module-side half of the call-site design.

**Org-mode noweb.** `<<name>>` splices another block\'s body, `<<name(arg=val)>>` its evaluated result ([noweb](https://orgmode.org/manual/Noweb-Reference-Syntax.html)). *Mapping.* Named cells referenced from other cells by label; cheap given cells have labels and a shared state. Low priority.

**Pandoc readers and `pandoc.template`.** `pandoc.read(markup, format)` parses a string; `pandoc.template.compile`, `apply` and `meta_to_context` render doctemplates from Lua ([lua filters](https://pandoc.org/lua-filters.html)). *Mapping.* A fragment is a compile-time `pandoc.read`; keep `pandoc.read` for dynamic strings (the `quarto.markdown` helper in [survey part B](#survey-b)), and expose `pandoc.template.apply` so an `.lmd` module can own a doctemplate, which ties lmd to the listings machinery.

**Markdoc.** Covered in [survey part A](#survey-a); its typed, validated attributes (`matches`, `errorLevel`) and partials with variables are the model for argument validation and parameterised include.

**Lua templating (etlua, Lapis).** etlua installs a loader so `render("hello")` loads `views/hello.etlua` through `require`; `content_for(name, content)` fills named layout slots; Lapis widgets build HTML as nested calls with `capture(fn)` returning a string instead of writing to the buffer ([etlua](https://leafo.net/lapis/reference/etlua_templates.html), [html generation](https://leafo.net/lapis/reference/html_generation.html)). *Mapping.* etlua\'s loader is the exact precedent for an extension-specific `require` loader for a template dialect; `content_for` maps to named output slots a module fills and a layout or later cell consumes; `capture` is `{=: :=}` already.

### Where the three passes agree

Ranked by how many passes put it near the top and by author value:

1. **A markup-side call site with parsed `Blocks` as children and slots as values** (Astro, Svelte, Jinja `call`, Hugo paired shortcodes done right, MyST directives). All three.
2. **`require` of `.lmd` modules as explicit namespaces**, through the existing Rust-side loader, with distribution over `_extensions/` (MDX imports, Jinja `import`, Typst `import`, every Lua dialect). All three.
3. **An implicit prelude / autoload** (Pollen `pollen.rkt`, MDX `components` provider, Hugo lookup). Two, and it is what the include workaround already approximates.
4. **Fragment literal as a trailing call argument**, `callout{=: … :=}` (Typst content blocks, Lua `f{}`). One, but it is a small grammar change with a large readability gain.
5. **A parameterised `include` returning `Blocks`** (Typst `#include`, Markdoc partials, etlua `render`). Two.
6. **Element handlers and show rules from a cell** (MDX `components`, Hugo render hooks, Typst `show`, Pollen `root`). All three, lower in each list.
7. **Props schema with validation and located call-site diagnostics** (Markdoc, Astro `Props`, Hugo `errorf`, MyST plugin schema). All three, dependent on 1.
8. **Escaping and alternate fragment delimiters** (MDX `\{`, Astro `is:raw`, Scribble `|{ }|`). Two; needed to document lmd in lmd.
9. **A `quarto.markdown(str)` helper** for dynamic markdown (Hugo `RenderString`, pandoc `read`). Two.

## Design: loading `.lmd` modules through `require`

### What a module is

An `.lmd` file is **Lua in the lmd dialect**: a chunk that may contain `{= … =}`, `{=: … :=}` and `{=[ … ]=}` fragments, and that returns a value, by convention a table of functions. It is not a markdown document with cells. Reasons:

- `require` returns the chunk\'s value, which is the whole point of a module; a markdown-shaped file has no natural return.
- MoonScript, Fennel, Yuescript and Teal all take this shape: the dialect file is the unit of loading, and the loader compiles it on the way in.
- The markdown-shaped prelude already exists as the include workaround, and a project-level autoload ([Phase 3](#phase-3)) can make it official without inventing a second module kind.

```lua
-- components/cards.lmd
local M = {}

function M.card(title, body)
  return {=:
::: {.card}
### `{lmd} title`

`{lmd} body`
:::
:=}
end

return M
```

````{.markdown}
```{lmd}
local cards = require("components/cards")
return cards.card("Hello", {=[ from a module ]=})
```
````

### Resolution

The scoped `require` gains `.lmd` candidates next to the `.lua` ones, so a module is named without an extension, the way MoonScript\'s loader sits ahead of Lua\'s and finds `.moon` first:

| `require(name)` tries, per directory on the stacks | kind |
|---|---|
| `<name>.lmd` | lmd |
| `<name>.lua` | Lua |
| `<name with . as />.lmd`, then `.lua` | lmd, Lua |
| `<name>/init.lmd`, then `init.lua` | lmd, Lua |

If both `foo.lmd` and `foo.lua` exist in the same directory the **`.lmd` wins** and a warning names both files (decision 3). The reason is `q2 lmd compile`: once it exists, a stale `foo.lua` left beside the `foo.lmd` an author is editing would silently win under the other order, and the author would hunt for why their edits do nothing. Loading the source of truth and warning about the leftover is the failure mode that explains itself. (Fennel puts its searcher after Lua\'s; MoonScript puts its ahead; the third survey pass preferred `.lua` first with an error, which the stale-file argument outweighs.) Dots in `name` keep their Lua meaning as path separators, so `require("cards.lmd")` is *not* the spelling (it would mean `cards/lmd.lua`); the scratch render above shows exactly that confusion.

The search roots are unchanged in Phase 1: the requiring module\'s directory, then the script-dir stack, which for a cell is the document\'s directory. Project-root and extension roots are [Phase 3](#phase-3).

### Desugaring on load, and where the code lives

The loader needs the desugarer, which is in quarto-core, and `require` is in pampa, which cannot depend on quarto-core. Two ways to close the gap:

- **(a) Inject a hook.** `register_scoped_require` (or a sibling `register_lmd_loader`) takes a `Arc<dyn Fn(&str, &Path) -> Result<String, Vec<DiagnosticMessage>>>` stored in the Lua registry; the `.lmd` candidate branch calls it with the file text and path before `lua.load`. quarto-core supplies the closure from `LmdRuntime::new`\'s caller (`LmdExecuteStage`), and also from `UserFiltersStage` and the shortcode engine so a plain Lua filter can `require` an `.lmd` module too. Nothing moves.
- **(b) Move the desugarer into pampa.** pampa already links `tree-sitter` and `tree-sitter-qmd` and owns the qmd reader the desugarer calls; adding `tree-sitter-lmd` and moving `desugar.rs`, `emit.rs` and `anchor.rs` (with `engine_cell_lang` duplicated or moved) puts the whole lmd *language* in one crate, with the two stages staying in quarto-core. The `wasm_lua.rs` tests could then cover desugar and load together.

Recommendation: **(a) first**, since it is a few dozen lines and leaves the stage code untouched, with (b) recorded as the cleanup to do when the desugarer stops changing weekly. Either way, the hook is also the implementation of **a Lua-callable `quarto.lmd.compile(src, name)`** (returns the desugared Lua as a string, or raises with the located message), which is the building block the user\'s second option asked for and which lets a Lua author write their own loader or compile a fragment held in a data file at run time.

### Environment, caching, and what a module sees

Unchanged from the `.lua` path: the module runs in a sandboxed environment inheriting the globals (so `pandoc`, `quarto`, `meta` and `FORMAT` are visible; `meta` is the *requiring document\'s* metadata, which is what a component wants), its directory is pushed on the require-private module-dir stack so its own `require`s resolve beside it, and the result is cached by resolved path, so a module shared by three cells runs once per document. Two cells in two documents get two Lua states and two runs; cross-document caching is a project-level question ([Phase 3](#phase-3)).

### Errors from inside a module

The module path is the chunk name (`@<path>`), and the desugarer runs in the line-preserving emit mode, so Lua\'s `cards.lmd:12:` is the line in the module file, in the message and in every traceback frame, with no padding needed (the module starts at line 1). Three refinements, in order of value:

1. A desugar failure (Lua syntax error, markdown error inside a fragment, unsupported node) surfaces as a Lua error at the `require` call, so the cell\'s diagnostic underlines the `require` line and the message carries the module\'s own `file:line:col` and the reader\'s title. This is what Fennel and MoonScript do and is enough to ship.
2. The module file is registered with the source map (the way include expansion registers an included file), so a markdown error inside a module fragment is a *located* diagnostic underlining the module source, and `By::filter("cards.lmd", 12)` attribution works for nodes a module builds. Requires the loader hook to return diagnostics rather than a string message.
3. Project-relative chunk names (bd-o7ujtnk3) matter more with modules than with cells, since module paths are long and Lua abbreviates past 60 bytes.

### File access: one chokepoint, so the VFS and capability policy apply

Every file the Lua side opens on this plan\'s behalf, the `.lmd` loader\'s candidates, `quarto.include`\'s target, a module\'s own nested `require`s, goes through the `SystemRuntime` handed to the Lua state (`runtime.file_read`), never through `std::fs` and never through Lua\'s `io`. That is already how the scoped `require`, `dofile` on WASM and the mediabag read, and it buys two things at once:

- **WASM.** The hub client\'s `SystemRuntime` is its virtual filesystem, so modules and includes resolve in preview with no second code path. The `wasm_lua.rs` tests pin it.
- **Capabilities.** `quarto-system-runtime` already has a `SandboxedRuntime` whose policy carries Deno-style `allow_read` / `deny_read` path patterns (`sandbox.rs`). A loader that only ever asks the runtime is governed by that policy for free, so an `--allow-read`-style flag on `q2 render`, or a per-extension policy, is a wrapper around the runtime, not a change to the loader. Phase 1 and Phase 3 each carry a test that a `SandboxedRuntime` denying the module\'s directory makes `require` and `quarto.include` fail with the policy\'s error, so the chokepoint cannot regress unnoticed.

The known hole is native Lua\'s own `io` library, which the full stdlib exposes to filters today; closing it is a separate decision about filter sandboxing, not part of this strand.

### Ahead-of-time compilation

A `q2 lmd compile <file.lmd>` subcommand that prints the desugared Lua (pretty mode) is cheap once the hook exists and serves three purposes: debugging ("show me the Lua") and publishing an extension as plain `.lua` so Quarto 1-style filters can consume it; the `.lmd`-wins rule above is what keeps its output from shadowing the source it was made from. It is a developer tool, not the loading mechanism; requiring a build step from authors is the thing MDX, Astro and Typst all avoid.

### WASM

The scoped `require` reads through `SystemRuntime`, the desugarer is pure Rust over a C grammar that already builds for wasm32 in quarto-core, and the hub client\'s virtual filesystem holds project files, so modules load in preview with no extra work. A `wasm_lua.rs` test that requires an `.lmd` module from a cell is the proof, and lmd\'s first WASM test at all.

## Component call sites (markup-side) {#component-call-sites}

Both surveys rank this first, above modules, because without it components are usable only by authors writing Lua. It is also the piece with the most design surface, so it gets its own strand and a short design note before code. The shape both surveys converge on:

```{.markdown}
::: {lmd="cards.card" title="Hello"}
Body written as *markdown*, parsed once, handed to the function as `Blocks`.

::: {slot="footer"}
A named slot.
:::
:::

An inline [badge]{lmd="ui.badge" tone="warn"} in a sentence.
```

desugaring to `cards.card{ title="Hello", children=<Blocks>, footer=<Blocks>, attr=<Attr> }` and `ui.badge{ tone="warn", children=<Inlines>, attr=<Attr> }` respectively, evaluated in the document\'s Lua state at the div\'s position in document order. Design points to settle in that note, with the survey\'s evidence attached:

- **Inner content is AST, never a string** (every Hugo `.Inner` pain point). A body that is one paragraph also arrives as `inlines`, mirroring `pandoc.Nodes`.
- **Slots are values, or functions** (Svelte snippets, Jinja `caller(item)`): a component that iterates calls `props.row(item)`.
- **Attribute coercion and validation** (Markdoc): div attributes are strings; a component may declare `props={ level={type="number", default=2} }` and the call site gets a located diagnostic.
- **Name resolution**: `lmd="cards.card"` is a Lua expression evaluated in the cell state (so `require` must have happened, or the autoload of [Phase 3](#phase-3) provides it). Whether a bare `lmd="card"` may also search an `lmd:` registry in metadata is open.
- **Relationship to shortcodes**: this is the paired shortcode Quarto never had. A `{{{< card title="x" >}}}…{{{< /card >}}}` spelling could desugar to the same call; the div form has the advantage that existing editors, the parser and the preview already understand it, and that attributes are already structured.
- **Relationship to `quarto.component`**: a call-site div whose function returns `quarto.component(...)` makes a React-bound element from markdown with no Lua in the page.

## Other quality-of-life items, with a disposition

| item | from | disposition |
|---|---|---|
| user guide for lmd | PR #855 | landing; **Phase 1** adds a modules section and trims its limitations list, each later phase does the same |
| footnotes inside fragments | gap | **Phase 2**; run note resolution over the fragment parse (tables closed by PR #850) |
| escape for a literal `{=` / `=}` in a fragment; a no-scan fragment form | MDX `\{`, Astro `is:raw` | **Phase 2**; displaying a whole cell is settled by the guide\'s `markdown` fence rule, this is for a fragment that quotes fragment syntax |
| `quarto.markdown(str) -> Nodes` for markdown held in metadata or data | Hugo `RenderString`, `markdownify` | **Phase 2**; a thin wrapper over `pandoc.read` |
| fragment literal as a trailing call argument, `callout{=: … :=}` | Typst content blocks, Lua `f{}` | **Phase 2**; one grammar rule plus desugarer support |
| alternate or length-varying fragment delimiters for bodies containing `:=}` | Scribble `|{ }|`, Lua long brackets | **Phase 2**, with the escaping item |
| `quarto.include(file, params) -> Blocks`, evaluating an `.lmd` or `.qmd` file with parameters | Typst `#include`, Markdoc partials, etlua `render` | **Phase 3**; the shortcode `include` is textual and runs before cells, this one is a value |
| per-directory implicit prelude auto-required into every document | Pollen `pollen.rkt` | **Phase 3**, as the autoload item |
| scoped show rules from a cell, `quarto.show(Header, fn)` for the rest of the document | Typst `show`, Pollen `root` | not now |
| module-registered directives and roles (`return { directives = {...} }`) | MyST plugins | later, with the call-site strand |
| named-cell references between cells | Org noweb | later, if asked |
| layout slots filled by modules (`content_for`) | etlua, Lapis | later |
| deferred evaluation (`context`) | Typst | not now; revisit with cross-reference needs |
| noisy fallback message when the scoped `require` misses on native | scratch render | **Phase 1**; prefer the scoped message, keep the stock one as a hint |
| WASM test for the lmd runtime | gap | **Phase 1**, with the module test |
| highlighting of `lmd` code (echo blocks, hub-client editor) | gap | **Phase 2**, if `quarto-highlight` can register the vendored grammar\'s Lua queries under `lmd`; otherwise highlight as `lua` |
| project autoload of modules (`lmd: modules:` in `_quarto.yml` or a document) | MDX `components` provider, Hugo partial lookup | **Phase 3** |
| modules contributed by extensions; project-root `require` paths | npm, Hugo modules, `_extensions/` | **Phase 3** |
| element handlers and layout from a module (`lmd.elements{ Header = … }`) | MDX `components`/`wrapper`, Hugo render hooks | not now; would be a filter registered from a cell, run after `lmd-execute` |
| per-language code-block hooks with fragment bodies | Hugo `render-codeblock-LANG` | not now |
| props schema with validation and coercion | Markdoc attributes, Astro `Props` | later, with the call-site strand |
| `lmd.when` / `lmd.map` flattening to `Blocks` | Solid `Show`/`For` | later, small; `pandoc.List` covers most of it |
| listing item query helpers (`where`, sort by key, group by) | Hugo `where`/`range` | later; bd-u1rxccq7 is one corner |
| per-component dependencies deduped at `require` | SFC scoped styles | later; `add_html_dependency` from a module already works, dedupe is the only addition |
| `q2 lmd compile` | `moonc`, `fennel --compile` | **Phase 1** stretch, developer tool; its output never shadows the source (`.lmd` wins) |
| explicit `memoize(fn, key)` | Hugo `partialCached` | only if a project-level Lua state appears |
| `.Store`/`.Scratch`, `dict`/`slice`, `baseof`/`extends`, content adapters, React `key` | Hugo, Jinja, React | not borrowing; Lua or Quarto templates already cover it, or it needs a project phase |

## Phases

This strand ends at Phase 3. Component call sites are a separate strand whose design notes are kept here; the element-handler and show-rule ideas are recorded in the disposition table as not now.

### Phase 0: survey and plan (done 2026-10-15)

- [x] Strand bd-76ogq4ew created.
- [x] Three survey passes (JSX/MDX/Astro/Markdoc; Hugo/Eleventy/Jinja/pandoc; Lua-dialect loaders and Typst/Pollen/Scribble/MyST/Org/etlua) merged into this document.
- [x] Scratch renders establishing what `require` does today for `.lua`, `.lmd` and the include workaround.
- [x] Reconciled with PR #855 (the lmd guide): branch base, guide section ownership, `Q-26` codes, its follow-up strands (2026-10-16).
- [x] Reconciled with PR #850 (structural desugaring contract, `Table` in fragments): the `Table` gap is closed, new entry points owe the set, branch-base note covers both PRs (2026-10-16).
- [x] Iterate this plan with the user; decisions recorded under [Decisions](#decisions) (2026-10-16).

### Phase 1: `require` of `.lmd` modules

Tests first, per the TDD rule; each bullet names its pin.

- [x] `pampa::lua::quarto_api`: a loader hook type and registry slot; `.lmd` candidates in the scoped `require` in the order of the resolution table; `.lmd`-over-`.lua` rule with a warning. Pins: `quarto_api::tests` for each candidate form, the precedence rule (the `.lmd` loads, the warning names both files), and the hook being called with the right path. *Landed as a generic `SourceDialect` trait (`pampa::dialect`, feature-independent) with Lua-side registration in `pampa::lua::dialect` (app data on the state); the loader tries every registered dialect\'s extension before `.lua`; chunk names are `@path` for all modules; the toy `Shout` dialect in `dialect::test_support` drives nine loader tests (commit `f45d54b5e`).*
- [x] `quarto_core`: supply the hook from `LmdExecuteStage`, `UserFiltersStage` and the shortcode engine (one constructor). Pins: `lmd_execute::tests` requiring a module that defines a component and uses it from a cell; a Lua *filter* requiring an `.lmd` module. *`quarto_core::lmd::LmdDialect` and `lmd::dialects()`; threaded through new `_with` entry points (`create_filter_environment_with`, `apply_lua_filter(s)_with`, `unified_filter::apply_filter(s)_with`, `LuaShortcodeEngine::new_with`, a `dialects` parameter on `LmdRuntime::new`) so the existing signatures stay as wrappers. Filter and shortcode-handler pins live in `lmd::dialect::tests`.*
- [x] Chunk naming and errors: `@<path>` chunk name, line-preserving emit, desugar failures as Lua errors at the `require` line with the module\'s position in the message. Pins: `lmd_execute::tests` for a runtime error on a module line, a markdown error inside a module fragment, and a nested `require` from a module. *A compile failure is a typed `ModuleCompileError` the execute stage recognises in the error chain and reports as `Q-26-8` (catalog entry, reference page, sidebar), located on the `require` line. Found on the way: a cell whose last statement is `return m.go()` is a Lua tail call, so the cell\'s own frame is absent from the traceback and the whole cell is the honest location; the tests avoid the tail call.*
- [x] `quarto.lmd.compile(src, name)` in Lua, built on the same hook. Pin: a Lua test compiling a fragment and `load`ing the result. *Generic: registering a dialect installs `quarto.<name>.compile`; pinned in `pampa::lua::dialect::tests` and in the `modules.qmd` fixture.*
- [x] Scoped-`require` miss message on native: the scoped candidates first, the stock message as the hint. Module-load failures get `Q-26` codes (continuing #855\'s catalog) with reference pages. *`Q-26-8` for a module that does not compile; a module that raises stays `Q-26-4` with the module\'s `path:line:` leading the message; the shadowing warning comes through `quarto.warn` and so carries the generic Lua-warning code.*
- [x] `docs/guides/authoring/lmd.qmd` (from #855): a "Reusable modules" section with an `examples/lmd/09-modules` project in the manifest and the `patterns.qmd` fixture; drop the modules bullet from "Current limitations"; update the "What a cell can use" rule. *The modules pattern is pinned by its own fixture, `modules.qmd` plus `modules/`, rather than by `patterns.qmd`, since it needs files beside the document; the limitations bullet now says what is left: project-wide directories, extension modules and autoload (Phase 3).*
- [x] `crates/pampa/tests/wasm_lua.rs`: a cell requiring an `.lmd` module from the VFS; and a native test that a `SandboxedRuntime` denying the module\'s directory makes `require` fail with the policy\'s error. *The WASM test uses a toy dialect through `apply_lua_filters_with` (pampa cannot see lmd); it runs in Linux CI, not locally. `SandboxedRuntime` turned out to delegate every file operation unchecked (a `TODO`), so `SecurityPolicy::can_read` / `can_write` and their enforcement on the read and write operations landed with this, which is what makes the denial test real.*
- [x] Snapshot fixture `tests/integration/fixtures/lmd/modules/` under `lmd_snapshots.rs`: a module, a consumer, an error inside the module. *`modules.qmd` plus `modules/{cards,broken,boom,shadowed}.lmd` and `shadowed.lua`; the harness now copies fixture subdirectories and its normalizer maps Lua\'s abbreviated chunk names for files under them.*
- [ ] Stretch: `q2 lmd compile <file>` printing the pretty desugaring. *Not started; `quarto.lmd.compile` covers the "show me the Lua" need from inside a document.*
- [ ] Update `2026-10-12-lmd-exploration.md`\'s open questions and this plan. *This plan is current as of the commits above; the exploration plan gets its pointer when Phase 1 merges.*

### Phase 2: documentation and fragment gaps

- [ ] Guide updates for each item below as it lands (the page exists from #855; each fix removes a line from "Current limitations").
- [ ] Footnotes inside fragments (resolve note definitions in the fragment parse, or run the fragment through the same early transforms the document gets); `Table` is done in PR #850. Fix the diagnostic location for shortcodes inside fragments (bd-onsqz7sv) on the way, and take bd-iqd6niat (numbers in inline cells) and bd-3h6iasmz (`OrderedList` attributes) if they are still open.
- [ ] Escaping: a literal `{=` or `=}` inside fragment markdown, and a fragment form that is not scanned for nested lmd code. Grammar and desugarer tests, corpus tests.
- [ ] `quarto.markdown(str)` returning a `Nodes`.
- [ ] Grammar: a fragment as a trailing call argument (`callout{=: … :=}`), with corpus tests and desugarer support; alternate delimiters decided together with the escaping item above.
- [ ] Highlighting for `lmd` blocks in output and in the hub client, or an explicit decision to highlight as `lua`.

### Phase 3: project-level and extension modules {#phase-3}

- [ ] Project-root resolution for `require` (leading `/`, the convention `components.src` already uses) and a `_extensions/<name>/` root addressable as `require("<name>/module")` (or a `contributes: lmd-modules:` key; decide with the user).
- [ ] Autoload: `lmd: modules: [...]` in `_quarto.yml` or a document\'s front matter, equivalent to a `require` into a named global before the first cell. Decide whether the include workaround stays documented as the prelude mechanism.
- [ ] `quarto.include(file, params)` returning `Blocks`: evaluates an `.lmd` file (or reads a `.qmd` fragment) in the document\'s state with a params table, distinct from `require`\'s module value. **Resolution and reading go through the Lua state\'s `SystemRuntime` only** (see File access above): relative to the including file, then the same roots `require` searches; pinned by a `wasm_lua.rs` test over the VFS and a `SandboxedRuntime` denial test. It is a new entry point under the structural desugaring contract (PR #850), so it applies the set and gets a row in `dev-docs/syntax-notes.md`.
- [ ] Pins through the real binary with a project fixture: a module in `_extensions`, a module at project root, an autoloaded module, a parameterised include.

### Later, own strand: markup-side component call sites

Not part of bd-76ogq4ew. Kept here so the design points and the decision that the div call site desugars to the `callout{=: … :=}` call form (decision 6) travel with the survey that motivated them.

- [ ] Design note settling the points listed in [Component call sites](#component-call-sites); iterate with the user.
- [ ] Desugar stage: a div or span with an `lmd=` attribute becomes a marked call site; the execute stage evaluates it in document order with `children` as `Blocks` or `Inlines` and slots from `slot=` children.
- [ ] Props schema and coercion; located diagnostics at the call site.
- [ ] Guide section and snapshot fixtures.

## Decisions {#decisions}

Recorded 2026-10-16 from the user\'s answers to the first draft\'s questions.

1. **Name: `lmd`.** No longer a placeholder. The extension is `.lmd`, the cell tag `{lmd}`, the guide in PR #855 already uses the name.
2. **Modules are Lua-shaped.** An `.lmd` file is a Lua chunk with fragments; its return value is the module. No markdown-shaped module kind (percent-script precedents noted and set aside); the prelude need is met by the Phase 3 autoload.
3. **`.lmd` wins over `.lua`, with a warning naming both files.** Chosen for the stale-compiled-file reason given under Resolution: with `q2 lmd compile` in the picture, the source of truth must be the one that loads.
4. **Modules first, call sites later.** Order as proposed; call sites get their own strand once modules exist to call.
5. **Desugarer stays in quarto-core for now; the loader gets it through an injected hook.** Moving the lmd language into pampa is recorded as a later cleanup.
6. **The call form `callout{=: body :=}` is in** (Phase 2), and the future markup-side div call site desugars to exactly that call, so the two spellings are one mechanism.
7. **Scope ends at Phase 3.** The former Phase 5 (element handlers, scoped show rules, per-language hooks) is not now; the former Phase 4 is the call-site strand.
8. **File access through the runtime only**, so the VFS serves WASM and the `SandboxedRuntime` policy can carry Deno-style capability flags later with no loader change. *Landed: `SandboxedRuntime` now enforces `allow_read`/`deny_read` and `allow_write`/`deny_write` on every file operation it wraps (it used to delegate unchecked); network, process and environment checks remain TODO there.*

## Open questions for the user

None outstanding. The six from the first draft are answered above; new ones go here as the phases raise them.

## Verification

For the survey step: this document renders under `q2 render --strict`. For Phase 1 and later: `cargo nextest run -p quarto-core -p pampa -E 'test(lmd)'`, the `lmd_snapshots` suite, `cargo xtask ts-test lmd` for any grammar change, `wasm_lua.rs` in CI, and `cargo xtask verify` before each merge.
