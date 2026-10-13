---
title: 'React components bound to document elements in `format: html` (bd-react-components-a8flgldm)'
date: 2026-10-13
description: 'Assessment and design for letting a .qmd bind React components to elements of its HTML output: the pandoc-rendered markup is the no-script version, a small ESM runtime mounts the component over it, components come from project files or npm, and the binding is declared in metadata or on the element. Designed to compose with lmd and type: metadata listings and to share one component contract with the q2 preview renderer.'
status: in-progress  # S1 and S2 landed 2026-10-13; S3 next
braid:
  strand: bd-react-components-a8flgldm
---

**Strand:** bd-react-components-a8flgldm (exploration). **Related:** bd-lmd-exploration-jqv2nbl4 (lmd, PR #823), bd-2oqen84t (lmd listings, PR #826), bd-j3764r9a (preview/render DOM parity epic), bd-kltzdhle (q2-preview as the hub-client default), bd-rpc3skz0 (allow-scripts + same-origin preview iframe decision), bd-4eyf (the artifact-store pattern for injected JS).

**Branch:** none yet. The plan is the deliverable of this session; spikes start after the open questions are answered.

## The idea

A `.qmd` author should be able to say "this element is a React component" without giving up anything about static authoring:

````qmd
---
components:
  - name: carousel
    src: carousel.tsx
  - name: sparkline
    src: sparkline.tsx
  - name: table
    src: npm:@acme/filter-table@2     # later; see § npm
---

### A page

::: {#carousel component=carousel}
- ![image 1](image1.png)
- ![image 2](image2.png)
:::

An inline component that behaves like a [12 15 9 22]{component=sparkline}.
````

Three properties define the feature:

1. **The static render is the no-script version.** `q2 render` writes exactly the HTML it writes today for the div and the span. A reader without JavaScript, a feed consumer, a search crawler and `--to markdown` all see the bullet list of images. The component replaces that markup only once its module has loaded and mounted. This is progressive enhancement rather than React's `prerender` + `hydrateRoot`: hydration requires the server markup to be the component's own first render, which q2 cannot produce without running the component at render time (§ Alternatives). What we get instead is stronger for a document tool: the fallback is the author's markdown, not a frozen snapshot of the component.
2. **One component contract for `q2 render` and the q2 preview.** The component receives the same props in both: the element's attributes, a JSON `props` payload, its rendered content as React `children`, and optionally the Pandoc AST of the node. In static HTML `children` is the captured DOM subtree; in the preview it is the registry-rendered React subtree. A component that ignores `host` works in both.
3. **No bundling by q2 at render time.** Components ship as ES modules, resolved through an import map. React is a single vendored ESM copy under `site_libs/`; project components are copied beside it; npm components come through the import map. `q2 render` stays node-free, as it is today.

The feature stands on its own, and it is the missing third piece for interactive listings: a `type: metadata` listing resolves the items, an lmd cell builds the fallback markup and the JSON props, and a component adds filtering, sorting or paging on top. § Composing with lmd shows that example.

## What exists today

Read from source on 2026-10-13. Line numbers are for the main checkout at `b976329ed`.

### Static HTML: how JS reaches the page

- **Two channels.** Linked files are artifacts with a `js:`/`css:` key (`crates/quarto-core/src/artifact.rs:53`, `dependency.rs:92`), emitted by `ApplyTemplateStage` as `<script src>` tags in `<head>` (`stage/stages/apply_template.rs:166`, `template.rs:278-280`), sorted by `(link_order, key)`; `Artifact::link_attribs` can add attributes such as `type="module"`. Inline HTML goes through the three include slots, `header`, `before-body`, `after-body`, via `append_with_sentinel` (`transforms/mod.rs:226`) or `quarto.doc.include_text`.
- **Vendored, embedded, predicate-gated.** Bootstrap, clipboard, tabsets, nav, List.js and mermaid each live under `resources/` as `include_bytes!`, and each has a stage or transform that registers the artifact when its predicate holds (`stage/stages/bootstrap_js.rs`, `clipboard_js.rs`, `tabsets_js.rs`, `transforms/quarto_nav_js.rs`, `listing_render.rs:176`, `mermaid.rs`). Project scope lands in `site_libs/`, single documents in `<stem>_files/` (`resource_resolver.rs:252`, `:366`).
- **No ES modules, no import map, no JSON data block.** The only `type="module"` in the tree is a marimo include carried verbatim. Data reaches client JS through `data-*` attributes (listings, tabsets, color mode) and through one inline `new List(...)` script (`project/listing/listjs.rs:100`). The listing-listjs plan says it outright: q2 has no inline-page-script mechanism.
- **No JS counterpart of `css:`.** `FormatCssTransform` (`transforms/format_css.rs:68`) is the copy-and-link model: push a `ResourceCopyIntent`, rewrite the metadata entry to a page-relative URL, relocate `_extensions/` files under `site_libs/quarto-contrib/`. `include-in-header` inlines file *contents*; `resources:` copies files but links nothing.
- **Render needs no JS runtime.** `render.rs:126` discovers an `esbuild` binary that nothing uses. Deno is spawned only by TS engine extensions at run time and by `q2 call build-ts-extension` at authoring time (`extension/build.rs`), whose doc comment states the precedent this plan follows: "Extension authors run this after editing TS source; `q2` never runs it during render." Node is used for `.js`/`.ts` pre- and post-render scripts (`project/render_scripts.rs:503`, `QUARTO_NODE`). **Deno is being phased out of q2** (decision of 2026-10-13: Deno's development has effectively stopped after its acquisition by Cloudflare, https://deno.com/blog/cloudflare), and the engine-host infrastructure will be ported to `node`. Nothing in this plan may add a Deno dependency; where a JS runtime is needed at authoring time, it is `node`.
- **Div and Span attributes.** The HTML writer keeps `id`, classes, `aria-*`/HTML5/RDFa keys, and prefixes every other key with `data-` (`crates/pampa/src/writers/html.rs:463-532`). So `{component=carousel}` already renders as `data-component="carousel"`.
- **Custom nodes vanish.** `Block::Custom` writes nothing (`html.rs:1778`); every custom node must be desugared to Divs and RawBlocks before the writer. The preview is the mirror image: it excludes those resolve transforms and dispatches on the custom node.

### The q2 preview: an existing, preview-only user-component mechanism

`render-components: [x.tsx]` in front matter is live in hub-client, `q2 preview` and the sandboxed preview (plan `2026-08-25-preview-spa-render-components.md`; `ts-packages/preview-renderer/src/utils/renderComponents.ts`, `tsxTranspiler.ts`, `q2-preview/entry.tsx:340-370`). What it is and is not:

- **Override by Pandoc tag or custom-node `type_name`, not by element.** The registry is one flat map keyed by `node.t` or `type_name` (`q2-preview/registry.ts:41-74`, `dispatchers.tsx:607-683`); user modules are merged over it by export name (`PreviewRoot.tsx:1430`). `export const Div` replaces every Div. The kanban fixture binds by class only by overriding all of `Div` and checking `classes.includes('kanban')` (`crates/quarto/tests/playwright-fixtures/q2-preview/render-components-kanban/kanban.tsx:14-20`).
- **Props are the raw AST.** `NodeArgs { node, setLocalAst, onNavigateToDocument }` (`framework/types.ts:149`); children are rendered on demand through `renderChildren` / `renderSlot` / `<Node>` from `window.__Q2_PREVIEW_RENDERER__` (`entry.tsx:129-148`), which the atomic-gate contract requires (`dispatch.tsx:395-401`).
- **No module system.** Babel standalone (about 2.9 MB, lazy in the SPA) transpiles the TSX with the classic runtime; the code loads from a `blob:` URL; `React` is `window.React`; bare and relative imports fail. No import map, no `react-dom` global.
- **Explicitly ignored by `q2 render`** (plan D6). The static output has no React runtime.
- **Sandbox.** Same-origin iframe with `allow-scripts allow-same-origin` in hub-client and the SPA (`iframe/Q2PreviewIframe.tsx:487`); the separate-origin sandboxed preview isolates by origin and defers a CSP because blob imports need `script-src blob:`. Raw-HTML `<script>`s never execute in the preview because content is inserted through `innerHTML`.
- **Attribute parity gap.** The preview `Div` keeps only `id`, `className`, `data-*`, `role`, `style` (`q2-preview/blocks/Div.tsx:10-73`); the native writer's `data-` prefixing of unknown keys has no preview counterpart, so `{interval=3000}` is `data-interval` in render and gone in preview. This matters for element-level binding and is a bd-j3764r9a child.

### Listings and lmd

Since PR #826 every resolved listing is published at `meta.listings.<id>` before `lmd-execute` and `user-filters-pre` (`project/listing/metadata.rs`, `stage/stages/listing_resolve.rs`), and `type: metadata` resolves without rendering. An lmd cell can build any markup from the items. What is missing on the JS side: `pandoc.json.encode` turns userdata (Inlines, the `meta` proxies) into `null` (`crates/pampa/src/lua/json.rs`), there is no helper that emits a JSON data block, and doctemplates have no JSON pipe at all. These are small and are part of § Composing with lmd.

### Prior art in this repo

There is no written record of the early React-binding discussion in `claude-notes/`; the nearest artifacts are the hub-react-todo demo (plan `2026-02-06-hub-react-todo-demo.md`: a standalone React app rendering a `#todo` div's AST over sync) and the preview `render-components` work above. This plan is the first document for the static-HTML half.

## Design

### D1. Two binding routes, one resolution

Both routes the request names are supported, and they resolve to the same thing: an element carrying `data-quarto-component="<name>"`.

**Metadata route.** A `components:` list, document or project level (project entries merge by `name`, document wins):

```yaml
components:
  - name: carousel          # required; the registry key, used by the element route too
    src: carousel.tsx       # project file (document-relative; leading / = project root) or npm:<pkg>@<ver>
    export: default         # optional; a named export of the module
    bind: "#carousel"       # optional selector: "#id" or ".class"; default "#<name>"
    content: dom            # dom (default) | ast | none; see D3
    props: { interval: 3000 }   # optional static props, merged under element props
```

**Element route.** `component=<name>` on any Div or Span attr: `::: {component=carousel}` or `[…]{component=sparkline}`. Props on the element: `component-props='{"interval": 3000}'` or, for the lmd case, a child `<script type="application/json" class="quarto-component-props">`. The element route needs an entry in `components:` to know the `src`; a bare `component=./carousel.tsx` shorthand (path as name) is allowed so a one-off does not need metadata.

**Resolution.** A `components-resolve` AST transform (`HtmlRender` profile only, late in `AstTransformsStage`, after user filters so Lua-built bindings count, before link rewrite and resource collection so `src` paths are rewritten and copied by the existing machinery where possible) does, for each binding:

1. finds the element: `#id` and `.class` selectors are matched on the AST (`Attr`), which is enough for v1; the element route is matched on the `component` key. Arbitrary CSS selectors are out of scope because the AST does not know the final DOM (§ Alternatives);
2. rewrites the attr to `data-quarto-component="<name>"` plus `data-quarto-component-props` when static props exist, so both writers (native, preview) carry it through unchanged, and so the DOM contract does not depend on the native writer's `data-` prefixing;
3. for `content: ast`, serializes the node to JSON and attaches it as a sibling `<script type="application/json">` (D3);
4. records the module for the manifest (D4) and registers the copy of a project `src` (D5).

Diagnostics (new `Q-` codes under a new subsystem): unknown component name on an element; `src` file missing; a selector that matched nothing; two bindings on one element; a binding on an element that is not a Div or Span; `component=` on a page whose format is not HTML (silently ignored, since the attribute is harmless elsewhere, but worth a note in docs).

### D2. The component contract

Published as a tiny TypeScript package, `@quarto/components` (types only, plus `defineComponent` for inference), under `ts-packages/`:

```ts
export interface QuartoComponentProps<P = Record<string, unknown>> {
  /** id, classes and the remaining key/values of the bound element */
  attrs: { id: string; classes: string[]; attributes: Record<string, string> };
  /** JSON props: component `props:` in metadata, overlaid by the element's props */
  props: P;
  /** The no-script content. Static HTML: the element's original DOM, reparented. Preview: the registry-rendered subtree. */
  children: React.ReactNode;
  /** The Pandoc AST of the bound node (content: ast); always present in the preview */
  node?: PandocBlock | PandocInline;   // from @quarto/pandoc-types
  /** The live element in static HTML; null in the preview, where React owns the element */
  host: HTMLElement | null;
}
export type QuartoComponent<P> = React.ComponentType<QuartoComponentProps<P>>;
```

The carousel from the sketch, for the record:

```tsx
import { useState } from "react";
import type { QuartoComponentProps } from "@quarto/components";
export default function Carousel({ children, props }: QuartoComponentProps<{ interval?: number }>) {
  // children is the <ul> of images; a real carousel would walk props.node or host instead
  const [i, setI] = useState(0);
  return <div className="carousel" onClick={() => setI(i + 1)}>{children}</div>;
}
```

Rules the contract fixes now so nothing paints us in: `children` is opaque (the component must not assume DOM nodes or React elements); structure comes from `node` (opt in with `content: ast`) or from `host` (static only, and then the component is static-only by construction); `props` is JSON, never AST; the module's default export, or the `export:` named export, is the component. `setLocalAst` and the preview's editing hooks are not part of the contract; a component that wants them is a preview override (`render-components:`), not a bound component.

### D3. Delivering the content: `dom`, `ast`, `none`

| `content:` | static HTML | preview | cost | for |
|---|---|---|---|---|
| `dom` (default) | runtime captures `host.childNodes` before mounting, hands them back as `children` through a `StaticContent` element that reparents the saved nodes into a ref | registry subtree as `children` | none | wrappers, decorators, "enhance this table" |
| `ast` | the transform attaches the node's Pandoc JSON (the wire format `@quarto/pandoc-types` already defines for the preview) as a sibling `<script type="application/json" data-for="…">`; the runtime parses it into `node` | `node` is what the dispatcher already has | page size, proportional to the bound subtree | components that re-render structured content: a carousel walking images, a listing re-sorting items |
| `none` | content dropped, `children` empty | same | none | components whose fallback is a placeholder |

`ast` is the preview-parity option: in both modes the component sees the same JSON. It is also the option that costs bytes, which is why it is opt-in. JSON inside `<script>` is escaped the way `listjs.rs:193` already does (`<` to `<`).

### D4. The runtime and the manifest

One vendored ESM file, `resources/js/quarto-components/runtime.js`, registered as a project-scoped artifact with `link_attribs = [("type","module")]` (module scripts defer, so `<head>` placement is fine). It runs after `DOMContentLoaded`:

1. reads the manifest, an inline `<script type="application/json" id="quarto-components-manifest">` in the `header` include slot: `{ components: { name: { module: "<page-relative url>", export, content } } }`;
2. `querySelectorAll('[data-quarto-component]')`, and for each host: `import(module)`, read props from `data-quarto-component-props` and from a child `script.quarto-component-props` (element wins over metadata), capture content per D3, then `createRoot(host).render(<Component …/>)`;
3. on any failure (import error, missing export, component throw) logs to the console and leaves the host untouched. The no-script version is also the error version.

An inline `<script type="importmap">` precedes the runtime in `<head>` (import maps must come before the first module script), mapping `react`, `react-dom`, `react-dom/client`, `react/jsx-runtime` to the vendored React (D5) and each npm specifier to its URL (D6).

### D5. Where React comes from

Vendored, pinned, offline, like every other JS asset q2 ships: an xtask (`cargo xtask vendor-react`) bundles `react`, `react-dom/client` and `react/jsx-runtime` from the repo's `node_modules` (React 19.2, already a dependency of hub-client and `preview-renderer`) into four small ESM files under `resources/js/react/<version>/`, with `react` external to the others so there is exactly one instance. React does not publish ESM builds on npm, which is why a build step in *this repo* is needed; it runs when we bump React, never during render. The files are embedded with `include_bytes!` and registered as project-scoped artifacts only when a page has at least one binding (the same predicate shape as `BootstrapJsStage`). About 200 KB minified, once per site.

### D6. Project modules and npm modules

**Project `src`.** Copied to `site_libs/quarto-components/<project-relative path>` (single document: `<stem>_files/quarto-components/`), preserving directory structure so relative imports between an author's files keep working. v1 copies the listed file and, when a transpiler is in the loop (D7), the files its relative static imports reach; without one, the author lists every file. `_extensions/` sources relocate the way `FormatCssTransform` relocates CSS.

**`npm:<pkg>@<version>`.** An import-map entry for `<pkg>` pointing at `https://esm.sh/<pkg>@<version>?external=react,react-dom` (the `external` makes esm.sh emit bare `react` imports that the map resolves to the vendored copy, so hooks work). The version is required; unversioned specifiers are a diagnostic. This is the one place the design reaches for a CDN, and it is contained: a later `q2 call vendor-components` can download the resolved module graph into `site_libs/` and rewrite the map, which is what the repo's vendored-not-CDN habit (mermaid, bootstrap) would want for production sites. `npm:` is deferred out of this strand (decision 5); nothing in S1 to S4 depends on it, and until the follow-up lands an `npm:` `src` is a diagnostic.

### D7. `.tsx` without bundling

Authors will write TSX. Three ways to get JS out of it, none of which bundles:

| option | how | pros | cons |
|---|---|---|---|
| A. author builds | `q2 call build-components` runs esbuild through `node` (`QUARTO_NODE`, the runtime the render scripts already use), with `react`/`react-dom` external, and writes `*.js` next to the source; render copies the JS. Precedent: `build-ts-extension`, minus Deno (that command is itself due for a port to node). | zero new Rust deps; full TS semantics | a build step and a node requirement for the author; stale-output footgun |
| B. transpile in Rust | `oxc_parser` + `oxc_transformer` + `oxc_codegen` (pure Rust, modular, builds for `wasm32-unknown-unknown`): strip types, compile JSX with the automatic runtime (`react/jsx-runtime`, resolved by the import map), rewrite nothing else. Per file, no bundling, no JS runtime. | `.tsx` just works in `q2 render` *and*, because the same crate runs in the hub-client WASM, the preview can drop its 2.9 MB Babel standalone (bd-j3764r9a adjacent win) | a new dependency family (compile time, binary size: to be measured); transpile-only means no type checking (fine; the LSP does that) |
| C. transpile in the browser | ship Babel to readers | none | 2.9 MB per page view. Rejected outright. |

Recommendation: S1 proves the architecture with plain `.js` ESM (no transpiler, no new dependency), S4 measures option B and decides. If B's cost is acceptable it becomes the default and A stays available for authors who want a real bundle; if not, A ships and B is filed. The user's stated leaning, distribution ESM files rather than bundles, is compatible with both.

### D8. The same components in the q2 preview

Owned by a sibling strand under bd-j3764r9a (decision 6), not by this one. The contract above is chosen so that half is additive:

1. **Dispatch.** A name-keyed component registry beside the tag-keyed one. The `Block`/`Inline` dispatchers check `attr` for `data-quarto-component` before the tag lookup and render `<C attrs props node children={renderChildren(args)} host={null}/>`. Bound components are leaves of the editing model (the atomic gate marks them read-only, like `IncludeExpansion`), which is the honest first cut.
2. **Loading.** The preview today loads user code from blob URLs with `window.React`. Bound components need real module resolution: the preview iframe gets an import map whose `react` entries point at a shim module re-exporting the iframe's own bundled React, and project modules are served by the hub/SPA (the sandboxed preview's service worker already serves same-origin GETs from the VFS). With D7-B the transpiler is the same Rust code; until then, Babel with the automatic runtime.
3. **Namespace.** `render-components:` (override a Pandoc tag or a custom node, preview-only) and `components:` (bind to an element, both outputs) are different things and keep different keys. The hub's own metadata namespace is untouched. Whether `render-components:` later folds into `components:` as `override: Div` is a question for after the sibling strand lands.

What this does not promise: components that edit their content back to source, or whose static fallback is live-editable. Those are the preview's override mechanism.

### D9. Composing with lmd and `type: metadata` listings

The interactive-listing shape the request asks for, end to end:

````qmd
---
listing:
  id: posts
  type: metadata
  contents: posts/*.qmd
components:
  - name: filter-table
    src: filter-table.tsx
---

```{lmd}
local items = meta.listings.posts.items
local rows = items:map(function(it)
  return { title = pandoc.utils.stringify(it.title), href = it["output-href"],
           date = it["date-iso"], categories = it.categories }
end)
return quarto.component("filter-table", { rows = rows }, {=:
| Title | Date |
|-------|------|
`{lmd} table.concat(items:map(function(it) return "| " .. pandoc.utils.stringify(it.title) .. " | " .. it.date .. " |" end), "\n")`
:=})
```
````

Two small Lua additions make this work: `quarto.component(name, props, blocks)` returns a `Div` with `data-quarto-component=name` wrapping `blocks` plus the props as a `script.quarto-component-props` RawBlock, encoding `props` with `pandoc.json.encode`; and `pandoc.json.encode` learns to encode `Inlines`/`Blocks` userdata (as their stringified text by default, which is what a table wants) instead of `null`. The lmd `{=: … :=}` fragment is the no-script table; the component gets `rows` as JSON and renders the interactive one. (The markdown-table-in-a-fragment above is awkward; the lmd follow-up bd-0o82ly9c on string positions and a `pandoc.SimpleTable` constructor in a fragment would improve it, and are out of scope here.)

The same `quarto.component` helper is what a plain Lua filter uses, so the feature is reachable without lmd, mirroring the "metadata first, lmd as one consumer" decision of PR #826.

### D10. Security and privacy

Static output: the author's own JavaScript on the author's own site, same as `include-in-header` today; the runtime adds no capability. `npm:` loads third-party code from a CDN at page view, which is a per-site decision; the manifest records the pinned URL and the vendoring command (D6) is the way out. Preview: user components already run in the preview iframe; this plan adds import-map resolution, not new reach. bd-rpc3skz0 (same-origin + allow-scripts as the default) and the sandboxed preview's deferred CSP are unchanged by this plan and are the right place for that conversation; the one input from here is that a CSP must allow `script-src` for the import-map targets, which are same-origin for everything except `npm:`.

## Alternatives considered

- **React `prerender` + `hydrateRoot`.** Needs the component's first render at build time, which needs a JS runtime (node) in `q2 render` and would make the fallback a snapshot of the component rather than the author's markdown. Rejected for v1; a future "SSR mode" could be an opt-in `prerender: true` per component implemented through a node subprocess (the engine host, once it is ported from Deno to node), and nothing here forecloses it (the contract is what would be rendered on the server).
- **Bundling at render time** (esbuild/rolldown in-process or as a subprocess). Fast and robust module resolution, but it brings node or a large Rust dependency into every render and produces per-site bundles that defeat caching across pages. The user's leaning against it holds up; import maps give the browser the resolution we would otherwise bundle for.
- **Web components instead of React.** Framework-neutral and hydration-free by nature (custom elements upgrade in place, which is exactly the progressive-enhancement story). Worth remembering as a second target: a `define` runtime that upgrades `data-quarto-component` hosts into custom elements is a few lines, and a React component can be wrapped in one. Not the request, so not the first target; the contract (`attrs`, `props`, `children`, `node`) is framework-neutral on purpose.
- **Arbitrary CSS selectors for `bind:`.** The runtime could match any selector in the DOM, but then `content: ast` and the diagnostics cannot be computed at render time, and the preview could not dispatch on it. `#id`, `.class` and the element attribute cover the request; a `selector:` key for DOM-only, static-only bindings can be added later without touching the contract.
- **Reusing `render-components:` as is.** Override-by-tag is a different feature (it replaces the renderer for every Div); binding is per element. Keeping both keys is cheaper than teaching one key two meanings.
- **A `js:` metadata key** symmetric to `css:` (copy and link a plain script). Useful on its own and tiny, but it is not this feature; filed as a candidate follow-up rather than folded in.

## Spikes

Each spike is one commit with its tests and ends with this plan updated. S1 is the architecture proof; S2 and S3 fill in the contract; S4 is the TSX decision with code, taken only after S1 to S3 are tested; S5 is docs. The npm route and the preview half are not spikes of this strand (decisions 5 and 6).

- [x] **S1. Static end to end, `.js` only.** Done 2026-10-13 (branch `braid/bd-react-components-a8flgldm-react-components-bound-document`). `components:` with `name`, `src` (`.js` ESM), `bind: "#id"` / `.class`, `content`, `export`; the `components-resolve` transform; the manifest and import map in the header slot; the vendored React ESM (`cargo xtask vendor-react`, embedded, registered only on pages that bind); the runtime; copying of `src` into `site_libs/quarto-components/<project-relative path>`. Fixture `crates/quarto/tests/integration/fixtures/components-basic/` (a website with one `.js` counter that wraps its children). Tests: 31 unit tests across `components/config.rs`, `components/runtime.rs`, `transforms/components_resolve.rs` and `project/format_paths.rs`; 6 end-to-end pins in `tests/integration/components_render.rs` (host attribute and untouched content, head order and page-relative URLs, assets byte-identical under `site_libs/`, `--to markdown` sees no binding, a page without components ships nothing, Q-25-4). Browser: verified by hand in Chromium over `q2 preview --static` (component mounts over the static markup, clicks update state, console clean); an automated Chromium smoke for rendered output has no harness in the repo yet, see § S1 findings. The no-script criterion holds: the bound div's markup is the markdown rendering, and the pandoc leg never sees the binding.

- [x] **S2. Element route and props.** Done 2026-10-13. `component=<name>` on Divs and Spans (the author key is consumed and replaced by `data-quarto-component`; a path-like value such as `component=./w.js` declares the component on the spot), `component-props='{…}'` (validated as a JSON object, renamed to `data-quarto-component-props`), `props:` in metadata (a map, carried in the manifest as plain JSON, overlaid by the element's props in the browser), `quarto.component(name, props, content)` in Lua (a `Div`, or a `Span` for inline content, tagged with the host attribute and carrying the props as a `script.quarto-component-props` child; an element tagged this way counts as bound, and an undeclared name is Q-25-7), and `pandoc.json.encode` of AST values (`Inline`/`Block` and `Inlines`/`Blocks` lists become their plain text, a `meta` proxy becomes the plain JSON of the metadata under it, through a new `ConfigValue::to_plain_json`). New codes Q-25-7 (unknown component name on an element) and Q-25-8 (`component-props` not a JSON object). Fixture `components-listing`: a website whose index declares a `type: metadata` listing and an lmd cell that hands the items to a filter-table component with a bullet list as the no-script version; the basic fixture gained an inline span bound by the element route. Tests: 11 more unit tests (props parsing, manifest props, element route, pre-tagged elements, Q-25-7/8), 6 Lua tests for `quarto.component` and the encoder, 2 more end-to-end pins. Browser: both fixtures verified by hand (the table filters, the props script is consumed, the static list survives as `children`, the span host mounts with a span wrapper).

- [ ] **S3. `content: ast` and the types package.** The AST JSON sibling script, the `StaticContent` reparenting element, `@quarto/components` types with `defineComponent`, the carousel fixture walking `node` for its images. Decide the JSON shape here: pandoc JSON as `@quarto/pandoc-types` already defines it, including the custom-node wrapper the preview understands.
- [ ] **S4. `.tsx` decision (oxc spike).** After S1 to S3 are green. Measure option B: add `oxc_*` behind a feature, transpile the S1 fixture rewritten in TSX, build for wasm32, record compile-time and binary-size deltas and the transpile time per file. Decide B as default or A (`q2 call build-components` over node + esbuild) as default, record it here, implement the winner. Either way `.tsx` in `src:` works at the end of S4.
- [ ] **S5. Docs and diagnostics.** `docs/guides/authoring/components.qmd` (or a section of `extensions.qmd`), the `Q-` error pages, a line in the listings guide's "Listings as data" pointing at D9, the `render-components:` versus `components:` distinction in the preview docs, and a short "when to use `content: ast`" note aimed at authors who want the same component in the preview.

Rough size: S1 two to three days (the vendoring xtask and the runtime are new ground); S2 one day; S3 one day; S4 one to two days including the measurement; S5 one day.

**Filed outside this strand** (to be created when S1 lands, so they can reference the real module layout):

- *`npm:` sources through the import map* (D6, deferred by decision 5): esm.sh with `external=react,react-dom`, version required, plus the vendoring command. Until then an `npm:` `src` is a diagnostic naming the follow-up.
- *Preview half* (D8, sibling strand under bd-j3764r9a by decision 6): name-keyed registry and attribute dispatch in `preview-renderer`, import-map shim for `react` in the preview iframe, module serving from the VFS/SPA, the S1 and S3 fixtures in the q2-preview Playwright harness, DOM-parity opt-in once the Div attribute parity child is fixed.

## S1 findings (2026-10-13)

1. **Import map values cannot be bare relative paths.** `site_libs/quarto-react/react.js` is read as a bare specifier and the browser drops the entry with a warning, so every module import fails. Values must start with `/`, `./` or `../`; the resolver hands back the bare form for a root page, so `import_map_url` prefixes `./`. Found in the browser, not by the unit tests, which is the argument for an automated smoke.
2. **React publishes CommonJS only, and esbuild externals do not survive `require`.** Bundling `react-dom/client` with `react` external leaves `require("react")` as esbuild's dynamic-require shim, which throws in a browser. `build.mjs` resolves the shared specifiers to a tiny ESM shim module instead (`import * as m from "react"; export * from "react"; export default m`), so the CommonJS `require` links to one real `import` of the bare specifier and the import map supplies the single instance. esbuild's own `external` option also externalizes subpaths, which would have externalized `react/jsx-runtime` itself. React 19's production `jsx-runtime` imports nothing from `react` (symbols only), so that file is standalone.
3. **Named exports need a generated entry.** esbuild cannot enumerate a CommonJS module's exports, so each vendored entry is generated at build time with an explicit `export const { useState, ... } = React` list. `check.mjs` links the four files the way an import map would and renders a stateful component under jsdom; `cargo xtask vendor-react` runs both scripts, and `vendored_react_matches_build_info` pins the banners against `build-info.json` so a React bump that skips the command fails the suite.
4. **No browser harness for rendered output.** The Playwright setup in `hub-client` is bound to the hub server and the preview; `preview_static_e2e.rs` talks HTTP without a browser. The S1 browser check was done by hand through the DevTools MCP. A small Playwright config that spawns `q2 preview --static` on a fixture and asserts the `quarto-component:mounted` event is the right follow-up; filed as a strand when S2 lands, since S2's fixture (the lmd listing table) is the second page it would cover.
5. **A missing `src` file is a copy-drain diagnostic.** The merge-time marking rebases `components[].src` unconditionally (`MarkPolicy::Always`, like `include-*`); a file that does not exist is reported once by the resource-copy drain as Q-5-6 at the `src` entry, so there is no Q-25 code for it. `npm:` counts as an external URL to `is_external_url`, so marking leaves it alone and `parse_components` reports Q-25-3.
6. **Manifest key order is alphabetical** (`serde_json` without `preserve_order`), which is deterministic and what the snapshots pin.
7. **Dynamic `import()` inside the runtime resolves against the runtime module's URL**, not the page's, so manifest URLs are page-relative strings that the runtime turns absolute with `new URL(url, document.baseURI)` before importing.

## S2 findings (2026-10-13)

1. **The lmd listing example in D9 is simpler than drafted.** With `quarto.component` the cell is four statements: map the items to plain rows (`pandoc.utils.stringify(it.title)`, `it["output-href"]`, `it.date`, `it.categories`), build a `pandoc.BulletList` of one-line fragments as the fallback, and return `quarto.component("filter-table", { rows = rows }, { list })`. The markdown-table-in-a-fragment contortion is gone; the fixture pins the four-statement form.
2. **`pandoc.json.encode` needed three cases, not one.** Besides `Inline`/`Block` userdata, the `pandoc.Inlines`/`pandoc.Blocks` lists (tables whose metatable `__name` is the list kind) encode as one string rather than an array of per-inline strings, and a `meta` proxy encodes as the plain JSON of its subtree so `pandoc.json.encode(meta.listings.posts.items[1])` is an object. `ConfigValue::to_plain_json` is the one projection behind both the proxy case and metadata `props:`; it also gave `quarto-pandoc-types` a `blocks_to_plain_text` beside the existing inlines one.
3. **An element tagged by Lua satisfies the declaration.** `quarto.component` sets `data-quarto-component` itself, so the transform treats an already-tagged element like the element route: the declaration counts as bound (no Q-25-4) and an undeclared name is Q-25-7 at the element. Without this the lmd fixture warned about `#filter-table` matching nothing.
4. **Single-quoted attribute values carry JSON through the qmd reader and the HTML writer.** `[40]{component=counter component-props='{"start": 40}'}` arrives with the quotes escaped as `&quot;` in the attribute and parses back in the browser. The alternative, double quotes outside, ends the attribute early; Q-25-8's page says so.

## Decisions (2026-10-13)

The seven open questions of the first draft, answered by the user the same day. The design text above was written to match; this list is the record.

1. **Key names stand:** `components:` at the top level, `component=` on the element, `quarto.component` in Lua. Revisit if the contract grows a second framework.
2. **`content: dom` is the default.** It costs nothing and minimizes surprise. Authors who want the same component in the preview, or structured content, reach for `content: ast`; the docs (S5) say so.
3. **React is vendored**, built once by an xtask from the repo's `node_modules` (D5), consistent with mermaid and bootstrap.
4. **The oxc spike is worth running**, but only after the fundamental spikes (S1 to S3) are ready and tested. S4 is sequenced accordingly.
5. **`npm:` is deferred.** No CDN fetch in v1; the first `npm:` consumer waits for the vendoring command. Filed as a follow-up once S1 fixes the module layout.
6. **The preview half is a sibling strand** under bd-j3764r9a, not a spike here. This strand owns the contract (D2, D3) and the static implementation; the sibling owns dispatch and loading in `preview-renderer`.
7. **Selectors are `#id` and `.class` only.** No DOM-only `selector:` for now.

## Verification plan

- Per spike: `cargo clippy -p quarto-core -p pampa --all-targets -- -D warnings`, `cargo nextest run -p quarto-core -p pampa -p quarto -E 'test(component)'`, and `cargo xtask lint`.
- The listing and lmd snapshot suites must be unchanged by S1 to S3: a page with no `components:` and no `component=` attribute renders byte-identically.
- Before merge: full `cargo xtask verify` (the WASM leg matters from S4 on; the hub-client leg belongs to the preview sibling strand).
- Browser checks: the Chromium smoke in S1 for mount and no-script fallback. The q2-preview Playwright harness is the sibling strand's.
