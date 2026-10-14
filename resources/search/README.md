# Website search resources

Assets shipped with every HTML page of a website (or book) that has
search enabled (bd-eip0naf4). `crates/quarto-core/src/transforms/search_ui.rs`
embeds them with `include_bytes!` and registers them as the
`js:search:*` project artifacts, which land in `site_libs/quarto-search/`.

The search *engine* is not here: the Pagefind runtime (`pagefind.js`,
`pagefind-worker.js`, the per-language `wasm.*.pagefind` modules) is
written next to the index at `site_libs/quarto-search/pagefind/` by
`crates/quarto-core/src/project/search/writer.rs`, taken from the
`pagefind` crate's own output so runtime and index always match.
`quarto-search.js` imports it lazily on first use.

## `autocomplete.umd.js`

Algolia Autocomplete 1.19.1 (`@algolia/autocomplete-js`, MIT), the
widget framework for the search box and results panel. Byte-identical
to the copy Quarto 1 vendors at
`src/resources/projects/website/search/autocomplete.umd.js`; upstream
<https://github.com/algolia/autocomplete>. Size: 96,397 bytes.

Replacing it with a Quarto-owned overlay (accessibility) is tracked as
bd-d88bj5r1.

## `quarto-search.js`

Quarto's search UI: a port of Quarto 1's `quarto-search.js` with the
Fuse.js engine replaced by Pagefind, federated-site merging, and the
deviations listed in the file header. Not minified; diff-reviewability
wins. Reads its options from `<script id="quarto-search-options">`
(see `search_ui.rs::options_json`) and the site-root offset from
`<meta name="quarto:offset">`.

## `autocomplete-preset-algolia.umd.js`, `search-by-algolia.svg`

The Algolia backend (`website.search.algolia`): the Autocomplete
preset that queries an Algolia index (`@algolia/autocomplete-preset-algolia`,
MIT; 6,352 bytes) and the "Search by Algolia" logo shown with
`show-logo: true`. Both byte-identical to Quarto 1's copies in the
same directory. Shipped only when the Algolia block is complete; the
`algoliasearch` client itself loads from the CDN (Quarto 1 parity).

## Styling

`resources/scss/html/templates/quarto-search.scss`, compiled as a Sass
layer of every Bootstrap theme (`quarto-sass::load_search_layer`).
