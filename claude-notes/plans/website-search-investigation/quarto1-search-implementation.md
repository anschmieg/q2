# Quarto 1 website search — implementation notes (bd-eip0naf4)

Read of `external-sources/quarto-cli` on 2026-10-14. Paths are relative to
`external-sources/quarto-cli/` unless noted. Main sources:

- `src/project/types/website/website-search.ts` (734 lines)
- `src/resources/projects/website/search/quarto-search.js` (1457 lines)
- `src/resources/projects/website/search/quarto-search.scss`
- `src/resources/schema/definitions.yml:916-990`
- EJS templates in `src/resources/projects/website/templates/`
- user docs: `external-sources/quarto-web/docs/websites/website-search.qmd`

## 1. User-facing configuration surface

### `website.search` (schema `definitions.yml:916-990`)

The schema is `anyOf: [boolean, object]`. The object form is `closed: true`, so unknown keys are rejected.

Defaults are resolved by `searchOptions()` (`website-search.ts:430-474`):
- If `website.search` is an object, its fields are read one by one.
- If it is `undefined` or truthy, a default config is returned (466-471).
- If it is `false`, the function returns `undefined`. Nothing is then injected: no dependency, no header include, no navbar or sidebar box. The index is still written (see §2).

| Key | Type | Effective default (code) | Effect |
|---|---|---|---|
| `location` | `navbar` \| `sidebar` | `navbar` if the site has a navbar, otherwise `sidebar` (`searchInputLocation`, 568-588) | Where the widget is placed. Any value other than `"sidebar"` is treated as `navbar`. |
| `type` | `overlay` \| `textbox` | `overlay` for navbar, `textbox` for sidebar (`searchType`, 518-537) | `overlay` shows an icon button that opens a modal ("detached" mode). `textbox` is an inline input. |
| `limit` | number | **50** (`searchInputLimit`, 476-486) | Passed as Fuse's `limit` or Algolia's `hitsPerPage`. It caps **entries (sections)**, not documents. The schema description (line 929) and the docs both say "defaults to 20", which is wrong. |
| `collapse-after` | number (the code also accepts `false`) | **3** (`kDefaultCollapse`, 428). `false` disables collapsing (440-445). | The number of rows per document (counting the document card) before a "N more matches" toggle. The code comment says "show 2 sections" and the docs say "Defaults to 2"; in practice 3 means the doc card plus 2 sections. |
| `copy-button` | boolean | `false` (449) | Adds a clipboard icon in the input suffix. |
| `merge-navbar-crumbs` | boolean | `true` (schema 936-939) | Index time only. Read directly from `format.metadata.website.search["merge-navbar-crumbs"]` (`website-search.ts:258-259`). |
| `keyboard-shortcut` | string or string[] | `["f", "/", "s"]` (488-502) | A single key is wrapped into an array. |
| `show-item-context` | `tree` \| `parent` \| `root` \| boolean | `false` (504-516) | Shows breadcrumbs on document cards. `true` behaves like `tree`. |
| `algolia` | object | none | See §4. |

There is also an internal option, `panel-placement`, not in the schema: `"end"` for navbar and `"start"` for sidebar (451, 467). The interface (`SearchOptions`, 96-107) also lists `full-width` and `input-wrapper-width`, but nothing produces those values.

### `navbar.search` / `sidebar.search` (schema 1035-1037, 1093-1095)

Booleans. In `navbarEjsData` (`website-navigation.ts:1241-1248`) and `sidebarEjsData` (`website-navigation.ts:1026-1031`) the rule is:
```
search: navbar.search !== undefined ? navbar.search
       : searchOpts && searchOpts.location === "navbar" ? searchOpts.type : false
```
So the template variable ends up as `false`, `true`, `"overlay"` or `"textbox"`. An explicit `false` always wins (test fixtures in `tests/docs/search/issue-9905/`).

An explicit `true` is not the same as a type string. The sidebar and navtools templates check `=== "overlay"`, so `true` renders as the inline textbox. Gap: `navbar.search: true` with no `website.search` options still only renders if search is globally enabled, because the JS bails when `#quarto-search` is missing and the dependencies come only from `searchOptions()`.

### Per-page and other inputs

- **Per-page `search: false`** (schema `document-website.yml:1-6`, default `true`, html-doc formats). Removes the page and all of its `href#…` entries from the index (`website-search.ts:194-203`). Drafts get the same treatment when not visible under the current draft mode (`isDraftVisible(projectDraftMode)`).
- **`title`**: taken from the rendered DOM, not from metadata: `h1.title`, falling back to `main h1`, then the site title (225-237, 276-279).
- **`description` and `categories` do not feed into the index.** They live in `#title-block-header`, which is removed before text extraction (282-285).
- **Books**: `book.search` is copied to `site.search` (`src/project/types/book/book-config.ts:175`; `kBookSearch = "search"` in `book-constants.ts:16`). Book post-render calls `websitePostRender` (`book-render.ts:630-631`). Books use a different main selector when indexing (§2).
- **Forced inclusion**: the `.quarto-include-in-search-index` class (357-362), documented under "Disabling/Forcing Search".
- **i18n**: language keys at `src/resources/language/_language.yml:69-79`. Every `search-*` key from `format.language` is copied into `options.language` (`website-search.ts:613-618`):
  - `search-no-results-text` = "No results"
  - `search-matching-documents-text` = "matching documents"
  - `search-copy-link-title` = "Copy link to search"
  - `search-hide-matches-text` = "Hide additional matches"
  - `search-more-match-text` = "more match in this document"
  - `search-more-matches-text` = "more matches in this document"
  - `search-clear-button-title` = "Clear"
  - `search-text-placeholder` = "" (empty by default; the docs tell users to set it)
  - `search-detached-cancel-button-title` = "Cancel"
  - `search-submit-button-title` = "Submit"
  - `search-label` = "Search" (used in the EJS `title` and `aria-label`)

## 2. Index generation (`updateSearchIndex`, `website-search.ts:149-426`)

**When it runs.** Called from `websitePostRender` (`website.ts:418-433`) after the sitemap is updated. The output file list is first filtered to remove `404.html` (`website.ts:424-427`). Books reach it through `book-render.ts:631`.

**Output location.** `<outputDir>/search.json`: one flat JSON array, pretty-printed with 2-space indent (420).

**Incremental merge** (157-177):
- On an incremental render with an existing `search.json`, the existing array is parsed and used as the starting set.
- If parsing fails, it warns "Unable to read search index - it may be corrupt…" and does **not** regenerate.
- On a full render it starts empty.
- `updateDoc` (211-218) upserts entries **by exact `href`**. It does not delete stale anchors: if a page loses a section during an incremental render, the old `page.html#old-section` entry stays. Pages are only purged as a whole, and only through the `search:false` or draft path (198-201).
- The file is written only if the array is non-empty and its serialized form changed (419-424).

**Inclusion rules.** Every output file of the render is a candidate:
1. `search === false` or a hidden draft: purge the page and its entries, then skip.
2. Non-HTML output (`isHtmlFileOutput`): skip (206-208).

There is no check that the page is listed in navigation.

**Parsing and stripping** (221-300). The HTML file is re-read from disk and parsed with deno-dom. Then:
- `#title-block-header` is removed.
- `nav[role="doc-toc"]` is removed.
- All `script` and `style` elements are removed. `innerText` is avoided so CSS-hidden content (inactive tabs) stays searchable.
- **Code blocks, callouts, footnotes and margin content are not stripped.** Text is `textContent`, no whitespace normalization beyond `trim()`.

**Sectioning** (310-415), keyed only on **level-2 sections**:
- Collects `doc.querySelectorAll("section.level2,section.footnotes")`. h3 and deeper are folded into their enclosing level-2 section's text. `section.level1` gets no entry of its own.
- **If any such sections exist:**
  - Main selector is `main > section:first-of-type` for books and `main.content` for websites (315-317).
  - The **first element child** of main has its headings removed; its `textContent` becomes the start of the page text, and the element is removed.
  - Then `${main} > p`, `${main} > div.cell` and `main > p`, plus any `.quarto-include-in-search-index` elements, are appended to the page text and removed (337-362). Fix for issue 10285 "pre-section content"; see `tests/docs/search/issue-10285/check-index.ts`.
  - If there is page text, one page entry: `{objectID: href, href, title, section: "", text, crumbs}`.
  - Each section **that has an `id`** gets `objectID = href = "${href}#${section.id}"` and `section` = the h2's `textContent` (or `""`). Text is the section's `textContent` with the h2 removed. Empty sections skipped (375-398).
  - Footnotes become a separate entry, usually `#footnotes` with an empty section title.
  - Content in a `section.level1` outside a level-2 section and not a direct `p` / `div.cell` child of main is **not indexed**.
- **Otherwise** a single entry from `main.textContent` (399-415).

**Entry shape** (`SearchDoc`, 140-147):
```json
{ "objectID": "docs/x.html#anchor", "href": "docs/x.html#anchor",
  "title": "...", "section": "...", "text": "...", "crumbs": ["A","B","Page"] }
```
- `href` is relative to the output dir, forward slashes, no leading `/`.
- `objectID === href`, which keeps the file importable into Algolia.
- `text` goes through `encodeHtml`, which only replaces `<`→`&lt;` and `>`→`&gt;` (`src/core/html.ts:40-43,63-68`). `&` is not escaped.
- **No length cap on `text`.** Entire sections, including code output, verbatim.

**Crumbs** (239-275):
- `sidebarForHref("/"+href, format)` finds the page's sidebar (single unnamed sidebar; else sidebar whose `id` matches page `sidebar` metadata; else the sidebar whose contents contain the href; `website-shared.ts:389-413`).
- `breadCrumbs()` walks the sidebar tree root → page (`website-shared.ts:342-381`). A section without `href` borrows its first child's href. Items without `text` are dropped.
- If `merge-navbar-crumbs !== false`, `navbarItemForSidebar` (`website-shared.ts:417+`) finds a navbar item resolving to the same sidebar and unshifts its `text` unless already first (fix for #7803).
- No crumbs → field omitted from the JSON.

The comment at line 10 notes that the Fuse index is **not** prebuilt at render time.

## 3. Browser runtime (`quarto-search.js`)

**At module load** (1-21):
- `q` query parameter read into `kQuery`, then stripped from the URL with `history.replaceState`, before `DOMContentLoaded` because `quarto-nav.js` resolves `<a>` hrefs against `window.location`.
- `show-results` (`kResultsArg`) also read; when present the panel opens and the input is focused (458-461).

**Initialisation** (`DOMContentLoaded`, 23-462):
- No `#quarto-search` element → exit.
- Options read from `<script id="quarto-search-options" type="application/json">`.
- Class `type-overlay` or `type-textbox` added.
- `autocomplete()` from `window["@algolia/autocomplete-js"]` configured with:
  - `container: #quarto-search`, `panelContainer: "#quarto-search-results"`
  - `detachedMediaQuery`: `"all"` for overlay, `"(max-width: 991px)"` for textbox (92-93). Textbox mode becomes the detached modal below `lg`.
  - `openOnFocus: true`, `defaultActiveItemId: 0`
  - placeholder and translations from the language strings
  - `initialState.query` = incoming `?q`
- Body scroll closes the panel unless detached (448-456, 50 ms throttle; bug #7117 covers iOS).
- On open, `positionPanel` runs after 150 ms (998-1014); it has a bug (`inputEl.left` / `inputEl.offsetRight` are not DOM properties).

**Loading the index** (`readSearchData`, 714-746):
- Loaded **lazily on the first non-empty query** (`getItems`), not at page load or on focus.
- Under `file://` the script `window.alert`s once and returns `undefined`, so the search then throws.
- Otherwise fetches `offsetURL("search.json")` and `fuse.add()`s every doc into `new Fuse([], kFuseIndexOptions)`. Index built at runtime and kept in memory.

**Fuse.js configuration** (704-712):
```js
keys: [{name:"title",weight:20},{name:"section",weight:20},{name:"text",weight:10}],
ignoreLocation: true, threshold: 0.1
```
Each search passes `{isCaseSensitive:false, shouldSort:true, minMatchCharLength:2, limit}` (302-307). In Fuse 6 only `limit` is honoured as a `search()` option; the other three are constructor options and silently ignored. No `includeMatches`.

**Sub-index optimisation** (`fuseSearch`, 1398-1457):
- If a search takes > 125 ms (`kFuseMaxWait`) and returned fewer than `limit` results, a sub-Fuse is built from those results; later queries that `startsWith` that term reuse it. The code comments that Bitap is O(nm).

**Result items.** Each result → `{title, section, href: href+"?q="+query (not URL-encoded, inserted before #), text: highlightMatch(query, text), crumbs}`.

**Grouping and collapse** (`reshape`, 143-245):
- Grouped by href before `#`. Page-level entry moved to the front of its group.
- First item of each group → `document` card; remaining → `document-item` cards. At `i === collapseCount` a `document-more` toggle is inserted ("N more match(es)…" / "Hide additional matches").
- Expansion state in `state.context.expanded` as a list of `search-more-${n}` targets.
- The more item uses a sentinel `href` (`kItemTypeMoreHref`, a GUID; line 9) so the navigator ignores it.
- `validateItems` (595-628) checks required fields and renders an error card; its `!item.title == undefined` checks are buggy, so only a missing `href` is detected.

**Rendering** (767-996):
- Document card: icon `bi-file-richtext`, `p.search-result-title`, optional crumbs, `p.search-result-section`, `p.search-result-text` via `dangerouslySetInnerHTML`.
- Section card: `.search-result-doc-section`.
- Cards wrapped in `a.search-result-link href=offsetURL(href)`.
- Document card with no section → class `document-selectable`.
- Header shows "`count` matching documents".

**`show-item-context`** (826-845): `root` → `crumbs[0]`; `parent` → `crumbs[len-2]`; otherwise (`tree`/`true`) → `crumbs.join(" > ")` with class `search-result-crumbs-wrap`. Shown only when `crumbs.length > 1`.

**Snippet highlighting in results** (`highlightMatch`, 1018-1102):
- Case-insensitive `indexOf` of the **whole query**; first hit wrapped in `<mark class='search-match'>`.
- Snippet clipped to ~50 chars before the match (backing up to a space, comma or colon) and ~200 after, with "…".
- If the query does not occur literally (fuzzy / multi-word), the full text is returned unhighlighted; CSS clamps to 2 lines.

**Navigation:**
- `navigator.navigate` uses `location.assign`; new tab / new window supported (246-265).
- The `item` template rewrites `item.href` to add `?q=encodeURIComponent(query)` when missing (Algolia path; Fuse hrefs already contain `?q=`).

**Highlighting on the target page** (56-76, 1104-1317). Custom code; mark.js is not used.
- `highlight(query, main)` collects leaf nodes and runs a streaming character matcher.
- Query lowercased with whitespace collapsed. Document whitespace must be present wherever the query has whitespace, extra whitespace ignored, so matches span inline formatting and syntax-highlighting spans.
- Matching text wrapped in plain `<mark>`.
- On `pageshow` (registered after tabsets.js so it wins), `openAllTabsetsContainingEl` activates Bootstrap tabs containing marks unless the active pane already has one.
- No hash → `scrollToFirstVisibleMatch` scrolls to the first mark (smooth, centered). With `?q=…#anchor` the browser's anchor scroll is used.
- Editing the query → `clearHighlight` removes marks (78-86).

**Keyboard shortcut** (396-420): `keyup` listener checks `kbds.includes(event.key)`; ignored when focus is in `input`/`select`/`textarea`/`button`/`option` or contentEditable. Calls `window.quartoOpenSearch()` (390-394), which closes/reopens the panel and focuses the input. The collapsed-sidebar search button calls the same (`nav-before-body.ejs:84`).

**Copy button** (`showCopyLink`, 630-696): on each state change a `button.aa-CopyButton` with `bi-clipboard` is inserted into `.aa-InputWrapperSuffix`; uses ClipboardJS to copy the current URL with `q=<query>&show-results=1`, flashes `bi-check2`. Shown only with a query and `copy-button: true`.

**Dark mode.** No JS dark-mode logic. `quarto-search.scss` is a Sass layer on the **bootstrap** dependency (`websiteSearchSassBundle`, 591-602), so it is compiled into both `bootstrap.min.css` and `bootstrap-dark.min.css` (`src/command/render/pandoc-html.ts:139-192`), using theme variables `$body-bg`, `$input-*`, `$popover-bg`, `$popover-body-color`, `$component-active-bg`/`-color`, `$navbar-fg`, `$sidebar-fg`, and tone-aware helpers `theme-dim` / `theme-fade` (`_bootstrap-functions.scss:2,56`).

Sass defaults (`quarto-search.scss:1-6`): `$quarto-navbar-search-input-width: 180px`, `$quarto-sidebar-search-input-width: 100%`, `$quarto-search-results-width: 400px`, `$quarto-search-collapse-icon-size: 26px`.

SCSS bug: `media-breakpoint-up(med)` at line 603 refers to a nonexistent breakpoint (`quarto-nav.scss` has the same mistake). Navbar ordering (`$navbar-search-order`) and `.sidebar #quarto-search` rules are in `navigation/quarto-nav.scss:168-186,269-275,458`.

## 4. Algolia path

**Server side** (`algoliaOptions`, 539-566): copies `application-id`, `search-only-api-key`, `index-name`, `params`, `index-fields`; converts `analytics-events` and `show-logo` to booleans; adds `libDir`.

`websiteSearchDependency` (665-686): adds `search-by-algolia.svg` if `show-logo`; adds `autocomplete-preset-algolia.umd.js` only if id, key and index are all present, else warns "Algolia search is misconfigured…".

`websiteSearchIncludeInHeader` (622-633): pushes a CDN `<script>` for `algoliasearch@4.5.1/dist/algoliasearch-lite.umd.js` (732-734). If `analytics-events`: also the search-insights IIFE loader (`window.aa`; unpinned CDN URL) and `@algolia/autocomplete-plugin-algolia-insights` (unpinned). Both go through `scriptTagWithConsent(cookieConsent, "tracking", …)` (`website-analytics.ts:65-83`), emitting `type="text/plain" cookie-consent="tracking"` when cookie consent is on. `cookie-consent-enabled` is written into the options JSON (628).

**Client side:** `getItems` calls `algoliaSearch()` (1337-1396). `getAlgoliaResults` (from the preset) runs one query with `params: {hitsPerPage: limit, clickAnalytics: analytics, ...params}`. `transformResponse` remaps `href`, `section`, `title`, `text`, `crumbs` using `index-fields` (schema allows only the first four), then `highlightMatch`. Results go through the same `reshape`, templates, grouping and page-highlighting. That is how the UI is shared.

**Insights** (`configurePlugins`, 464-536): plugin wrapped in `deferredLoadPlugin` (543-593) so it works once consent loads the scripts. `aa("init", {appId, apiKey, useCookie: cookieConsentEnabled, userToken})`; `userToken` is a random 20-char session token when consent is off. `onItemsChange` chunks `viewedObjectIDs` events into 20-item batches.

**Logo footer** (347-365): `offsetURL(`${libDir}/quarto-search/search-by-algolia.svg`)`.

The docs present `search.json` as directly uploadable to Algolia; `objectID` exists for that reason.

## 5. Pain points visible in the code

- **Whole index loaded and indexed in the browser.** `fetch` of the full `search.json`, then N `fuse.add` calls on the first keystroke. No prebuilt or serialized index, no sharding. Full uncapped section text (including code) goes into the JSON.
- **Fuse performance.** The O(nm) Bitap comment and the 125 ms sub-index heuristic (1403-1455), only applied when fewer than `limit` results came back.
- **`limit`** caps sections, not documents. Docs/schema say 20; code uses 50.
- **`collapse-after` default** (3) conflicts with the docs (2).
- **Fuse options ignored** when passed to `search()` (302-307).
- **Fuse hrefs carry an unencoded query** in `?q=` (1426-1437).
- **Snippet highlighting is literal-substring only**; fuzzy / multi-word matches show without highlight. `encodeHtml` does not escape `&`, and highlighting runs on escaped text, so queries with `<` / `>` do not line up.
- **Incremental renders do not prune stale section anchors.** Index not rewritten when empty; no recovery from a corrupt index without a full render.
- **Content lost by sectioning.** Content in `section.level1` outside `level2` is lost; h3+ get no anchors.
- **Vendored but unused library.** `autocomplete.min.js` (autocomplete-js 1.4.0) is vendored but never referenced; the file used is `autocomplete.umd.js` (1.19.1).
- **Version pins** (`configuration` file, 59-62): `AUTOCOMPLETE_JS=1.19.1`, `FUSE_JS=6.6.2`, `ALGOLIA_SEARCH_JS=4.5.1`, `ALGOLIA_SEARCH_INSIGHTS_JS=2.0.3`. Vendoring script: `package/src/common/update-html-dependencies.ts:267-291,504-536`.
- **Smaller bugs:** `validateItems`, `positionPanel`, the `file://` alert followed by an error.
- **Other features strip search UI.** `src/resources/filters/llms/llms.lua:34` and `website-llms.ts:149-151` strip `#quarto-search-results` and `.quarto-search` from llms.txt output.

## 6. Assets injected and HTML templating

**Wiring** (`website-navigation.ts:265-283`):
1. `websiteSearchDependency(project, source)` returns `clipboardDependency()` (`clipboard.min.js`, `format-html-shared.ts:76-84`) then the `quarto-search` dependency with scripts in order: `autocomplete.umd.js`, `fuse.min.js`, `quarto-search.js`, and `autocomplete-preset-algolia.umd.js` when Algolia is configured (659-680). The dependency also emits `<meta name="quarto:offset" content="../">` (page's relative offset to site root, or the absolute `site-path` for `/404.html`; 690-695). `offsetURL()` (1321-1335) uses it to build `search.json` and result URLs.
2. `websiteSearchSassBundle()` pushes the `quarto-search.css` layer into the bootstrap CSS.
3. `websiteSearchIncludeInHeader` writes a temp file with `<script id="quarto-search-options" type="application/json">{...}</script>` (prepended, 636-640) and the Algolia CDN/insights scripts, passed as `include-in-header` (`website-navigation.ts:406`).

Files land in `site_libs/quarto-search/` (`website.ts:116` registers `quarto-search` as a format lib dir).

**Options JSON contents:** `location`, `copy-button`, `collapse-after`, `panel-placement`, `type`, `limit`, `algolia`, `keyboard-shortcut`, `show-item-context`, `language`.

**Templates:**
- `navsearch.ejs` (1 line): `<div id="quarto-search" class="<%= classes %>" title="<%- language['search-label'] %>"></div>`. Autocomplete renders everything inside.
- `nav-before-body.ejs:10`: always emits `<div id="quarto-search-results"></div>` (panel container for textbox mode). Navbar includes `navsearch` when `nav.navbar.search` is truthy: before the toggler when collapsible (29-31), else after the tools (47-49, 53-55). Navbar `navtools` is always called with `search: false`. In the collapsed-sidebar secondary nav, if `nav.sidebar.search`, a `button.quarto-search-button` with `bi-search` calls `window.quartoOpenSearch()` (83-87).
- `sidebar.ejs`: for `sidebar.search` other than `"overlay"`, renders `<div class="sidebar-search">` wrapping `navsearch` (68-75), hidden below `lg` by SCSS. For `"overlay"`, search is placed among the sidebar tools; `toolsLocation` chosen from logo, title, search or fallthrough (16-32).
- `navtools.ejs:11-13,54-56`: when `search === "overlay"`, search counts as a tool and renders `navsearch` with classes `quarto-navigation-tool px-1`.

**Tests worth porting:**
- `tests/smoke/website/search.test.ts` (navbar/sidebar opt-out)
- `tests/docs/search/issue-10285/check-index.ts` (pre-section text present in a book's `search.json`)
- `tests/integration/playwright/tests/html-search-highlight.spec.ts`
- `tests/integration/playwright/tests/html-search-tabsets.spec.ts`
