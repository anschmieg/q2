# 09-interactive-listing — A listing, an lmd cell, and a React component

Six blog posts, a listing that resolves them without rendering anything,
an `{lmd}` cell that writes the page from the listing's items, and a
React component (in TypeScript, no build step) that filters the same
items by tag in the browser. Without JavaScript the page shows the
year-by-year list the cell wrote.

## What this demonstrates

- **`listing: type: metadata`.** The listing in `index.qmd` matches,
  sorts and publishes the posts at `meta.listings.posts` and renders
  nothing itself (no container, no paging script). The categories
  sidebar, RSS and dependency tracking would still work.
- **An `{lmd}` cell reading the listing.** The cell in `index.qmd`
  maps the items to plain rows (`title`, `href`, `date`, `year`,
  `tags`, `description`), counts the tags, and builds the no-script
  markup with markdown fragments: a `### year` heading per year and a
  bullet list of links. Fragments (`{=: … :=}`) are markdown inside
  Lua; `` `{lmd} … ` `` inside a fragment is Lua inside markdown.
- **`quarto.component(name, props, blocks)`.** The cell returns the
  bound element: a div carrying `data-quarto-component="tag-filter"`,
  the props as a JSON script, and the no-script blocks.
- **A component in TypeScript.** `tag-filter.tsx` is declared under
  `components:` and transpiled when the page renders; it imports
  `react` through the page's import map (a copy of React shipped under
  `site_libs/quarto-react/`) and its types from `@quarto/components`.
  With `content: none` the static list is replaced by the component
  once it mounts; `content: dom` would hand the list back as
  `children` instead.

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/websites/09-interactive-listing
cargo run --bin q2 -- preview --static examples/websites/09-interactive-listing
```

The second command serves `_site/` and opens it in a browser. A plain
file open does not work: the page loads ES modules, which browsers
refuse from `file://` URLs.

## What to look for

In the rendered `_site/index.html`:

1. **The host element.** `grep 'data-quarto-component="tag-filter"'`
   finds the div the cell returned, followed by the
   `<script type="application/json" class="quarto-component-props">`
   with the six posts and the tag counts, then the `<h3>` year
   headings and the list.
2. **The head.** An `<script type="importmap">` mapping `react` and
   friends to `site_libs/quarto-react/`, the
   `quarto-components-manifest` JSON naming `tag-filter` with its
   module URL and `"content":"none"`, and the runtime's
   `<script type="module">`.
3. **The transpiled module.** `site_libs/quarto-components/tag-filter.js`
   has no type annotations and imports `react/jsx-runtime`; the `.tsx`
   source is not copied.

In the browser: tag buttons with counts (`lua 4`, `notes 3`, …);
pressing one narrows the list and the count line reads
`4 of 6 posts tagged lua`; pressing a second one narrows further
(`tagged lua and react`); `clear` restores the six. With JavaScript
disabled, the year-by-year list is shown instead.

## Things you may notice

- Post links are `posts/<name>.html` in both the static list and the
  component's props: the cell wrote `pandoc.Link(it.title, it.path)`
  and `it["output-href"]`, and the link rewriter and the listing
  machinery produced the output paths.
- The tag buttons use Bootstrap's button classes, which the site's
  theme already ships; a component can rely on the page's CSS.
- The component never sees the markdown: `content: none` drops the
  static list on mount. Choose `content: dom` to keep it as
  `children`, or `content: ast` to receive the element's Pandoc AST.
