# Where website search plugs into Quarto 2 — infrastructure map (bd-eip0naf4)

Read of the q2 tree at `main` @ `80faa63f4` on 2026-10-14. There is no website
search yet. The pieces it would plug into already exist: a navbar `search:`
flag and placeholder, localized search strings, and a "capture each page
during render, then write one project file afterwards" pattern used by
llms.txt. The main constraint is the browser preview: it renders only the page
you're on (§5).

## 1. Website rendering pipeline

**Two passes.** Driver: `ProjectPipeline` in `crates/quarto-core/src/project/orchestrator.rs` (module docs 10-29).
- **Pass 1** runs the head stages from `build_head_stages()` (`crates/quarto-core/src/pipeline.rs:311-358`): SourceConversion, ParseDocument, MetadataMerge, LanguageResolve, IncludeExpansion, IncludeResolve, **ListingItemInfoStage**, **DocumentProfileStage** (the checkpoint), LinkResolution. Result: one `DocumentProfile` per file, collected into `ProjectIndex` (`project/index.rs`: `profiles()`, `lookup_by_source`, `lookup_by_href`).
- **Pass 2** runs the full stage list per document. Tail after the head stages: `build_html_pipeline_stages_with_options` (`pipeline.rs:360-454`): UnwrapProfile → engine → CompileThemeCss → Bootstrap/Clipboard/Tabsets JS (native only) → ListingResolve → user filters → AstTransforms → … → RenderHtmlBody → ApplyTemplate. The AST transform list is `build_transform_pipeline` (ends ~`pipeline.rs:2570-2600`); phases (`transform.rs:72`): Normalization, Crossref, Navigation, Finalization.
- **Project-level hooks** on the `ProjectType` trait (`orchestrator.rs:331-468`): `pre_render(project, index, runtime)` after Pass 1; `post_render(project, index, output_paths, project_artifacts, resolver, runtime, diagnostics)` after Pass 2; `post_resources(...)` after the resource copy (native only in practice).

**`WebsiteProjectType::post_render`** (`orchestrator.rs:529-592`): on both native and WASM runs `flush_project_artifacts`. Native only: `copy_favicon`, `copy_navbar_logo`, `copy_footer_images`, `write_sitemap`, `write_robots_txt`, `write_alias_redirects`, `listing::post_render_upgrade::substitute_listing_placeholders`, `listing::feed::complete_staged_feeds`. `post_resources` (`:602-625`) calls `llms_post_render::write_llms_artifacts`. These live in `project/website_post_render.rs`, all `#[cfg(not(target_arch = "wasm32"))]`, writing into `output_dir`.

**Existing "collect from every page, emit one project file" mechanisms.** llms.txt is the closest model:

1. **sitemap** (`website_post_render.rs:499 write_sitemap`) uses only `ProjectIndex` profiles plus `output_paths`. Metadata-only.
2. **llms.txt** (`transforms/llms.rs` + `project/llms_post_render.rs`), two halves:
   - **Capture:** `LlmsCaptureTransform` sits at the tail of the Finalization phase (`pipeline.rs:2570-2578`, `llms.rs:163-260`); crossref numbers and link rewrites are already resolved there. It clones `ast.blocks`, builds a cleaned view (`build_llms_view`: unwraps sectionize divs and restores ids onto headings, drops raw HTML), serializes it, and stores it as a **path-less, Project-scoped artifact**: `ctx.artifacts.store(format!("{LLMS_ARTIFACT_PREFIX}{md_href}"), Artifact::from_string(..).with_scope(ArtifactScope::Project))`. Path-less artifacts are skipped by every flusher and merged into the orchestrator's project `ArtifactStore` (`artifact.rs:354 drain_project_scoped`, `:384 merge_into_project`).
   - **Emit:** `write_llms_artifacts(project, index, project_artifacts, ...)` (`llms_post_render.rs:105+`) iterates `index.profiles()` and looks up each page's capture via `project_artifacts.get(key)`.
   - **Incremental renders:** for pages skipped this run it reads the previous run's on-disk output, guarded by a manifest at `.quarto/llms-manifest.json` (module docs 15-52). A search index needs the same handling: `RenderMode::Subset` (`orchestrator.rs:905-910`) re-renders only some pages, while profiles come from the on-disk Pass-1 cache (`project/profile_cache.rs`, `.quarto/cache/profiles/`).
3. **Listings** have a different shape: `ListingItemInfoStage` (before the checkpoint) feeds `DocumentProfile.listing_item` / `derived_listing`; `ListingResolveStage` resolves items from the Pass-1 `ProjectIndex` during Pass 2 of the host page. They read other pages' Pass-1 data during a per-page render rather than aggregating after rendering.

**`post_render_upgrade`** (`project/listing/post_render_upgrade.rs`, `substitute.rs`, `reader.rs`): the listing template writes placeholder comment markers; after Pass 2, `substitute_listing_placeholders` reads each output with `runtime.file_read`, reads referenced sibling HTML, extracts content with `scraper` (`reader.rs`), rewrites the host file in place. The module header (10-40) **explicitly forbids reuse for search**: "If a future feature (search indexing, social meta, etc.) needs sibling rendered content, it gets its own named step in `post_render`, not a hook into this module's machinery." `scraper` is native-only (`crates/quarto-core/Cargo.toml:140-146`), so reading HTML back cannot happen in WASM.

**`DocumentProfile`** (`document_profile.rs:668-995`, version 15): `source_path`, `output_href`, `format_id`, `title/subtitle/description/abstract: Option<Inlines>`, `authors`, `authors_structured`, `date`, `categories`, `keywords`, `image`, `draft`, `aliases`, `order`, `outline: Vec<TocEntry>`, `includes`, `nav_dependencies`, `body_link_targets`, resources, `listing_item: ListingItemInfo`, `derived_listing: DerivedListingValues`, `engine_resolution`, `comments`.
- `outline` is the heading tree. `TocEntry` (`pampa/src/toc.rs:144`): `id`, `content: Inlines`, `children`.
- `DerivedListingValues` (`:153-176`) holds only the first-paragraph description and first image; its doc comment says "other consumers — search, for one — want the body-derived text regardless."
- **No full body text.** The profile is extracted *before* engine execution, so code-cell output is absent.
- Available for every page at project level through `ProjectIndex` in `pre_render` and `post_render`, and to every Pass-2 page as `ctx.project_index`.

## 2. Navbar, sidebar, templates

**Data model:** `crates/quarto-navigation/`.
- `navbar.rs`: `Navbar` (`:146-176`) **already has `pub search: bool`**, parsed from `navbar.search` at `:233`. The doc at `:14` lists `search` among the Q1-mirrored keys. `tools-collapse` is parsed; no general `tools:` list yet; only `dark_mode_toggle: bool`, set by `NavbarGenerateTransform` (`transforms/navbar_generate.rs:100`).
- `Sidebar` (`sidebar.rs:394-411`) has **no `search` field**.

**HTML** is built in Rust by string building (`quarto-navigation/src/render_html.rs`).
- `navbar_to_html` (`:43-170`) emits `<div class="quarto-search"></div>` when `navbar.search` is true (`:128-130`, test `:1833`), then the `quarto-navbar-tools` div with the color-scheme toggle (`:140-155`).
- `secondary_nav_to_html` (`:290-320`) deliberately omits Q1's search button because `window.quartoOpenSearch()` does not exist yet (comment `:302`; test `:3255-3266`).

**Flow:** resolve → `navigation.navbar` meta (generate transform) → `NavbarRenderTransform` (`transforms/navbar_render.rs`) → `rendered.navigation.navbar` HTML string. Same generate/render pairing for footer, sidebar, secondary-nav, toc, breadcrumbs.

**Page template:** inline Rust constants in `crates/quarto-core/src/template.rs` (`FULL_HTML_TEMPLATE`, partials such as `QUARTO_HEADER_PARTIAL` at `:684`, registered in `builtin_html_partials()` `:698-712`). No on-disk HTML template dir; `scss/html/templates/` is SCSS only; listing templates are `include_str!` files under `project/listing/templates/`.

**Search-related strands:** bd-6cme "[websites] Sidebar search integration" (open) depends on "the separate search epic". `book/config.rs:72` lists `"search"` among website keys a book shares with websites.

## 3. JS/CSS asset injection

**Pattern:** a stage or transform stores a Project-scoped artifact whose key starts with `js:` (or `css:`) and has a `with_path`. `ApplyTemplateStage` (`stage/stages/apply_template.rs:166-167`, `collect_artifact_urls(ctx, "js:", ...)`) emits `<script src>` tags in **sorted key order**, so load order is chosen by key name. The resolver maps the path to `_site/site_libs/<path>`, or to `/.quarto/project-artifacts/` in the WASM VFS.
- Prototype: `stage/stages/bootstrap_js.rs` (docs `:9-60`, `BOOTSTRAP_JS = include_bytes!(…resources/js/bootstrap/bootstrap.bundle.min.js)`, key `js:bootstrap`). Siblings: `clipboard_js.rs`, `tabsets_js.rs`, transform `transforms/quarto_nav_js.rs` (keys `js:quarto-nav:headroom` / `js:quarto-nav:nav`).

**listjs is the closest analogue** (`transforms/listing_render.rs:169-195`): `LIST_MIN_JS = include_bytes!("…/resources/listing/list.min.js")` and `quarto-listing.js` registered as `js:listing:list.min.js` / `js:listing:quarto-listing.js` with `.with_path("listing/…")` and `ArtifactScope::Project`, only when a listing actually rendered. Per-listing inline init `<script>` from `project/listing/listjs.rs:init_script` as a `RawBlock`; author strings JSON-escaped (`<` → `<`, `script_json`).

**Dark mode:** `COLOR_MODE_JS = include_str!("../resources/js/quarto-color-mode.js")` inlined by the template (`template.rs:914, 1140`).

**Vendored tree:** top-level `resources/` with `js/{bootstrap,clipboard,headroom,quarto-nav,tabsets}`, `listing/`, `mermaid/`, `bootstrap-icons/`, `language/`, `scss/`. `resources/js/README.md` documents provenance/version/size per library and says a new library needs a "`BootstrapJsStage`-style stage". `dependency.rs` stores Lua/extension `HtmlDependency` files as `css:/js:{name}[:{version}]:{file}` at `libs/{name}/…`.

**WASM note:** the JS stages are cfg-excluded on wasm32 because the q2-preview pipeline drops `ApplyTemplateStage` (`pipeline.rs:516`, `Q2_PREVIEW_STAGE_EXCLUDED`). Preview chrome JS is injected separately via `?raw` imports in `ts-packages/preview-renderer/src/q2-preview/entry.tsx:48,65-66`. A search UI script would need the same two-sided wiring.

**SCSS:** `CompileThemeCssStage` (`compile_theme_css.rs`) calls `quarto_sass::assemble_theme_scss` (`crates/quarto-sass/src/compile.rs:72-118`), which pushes built-in layers unconditionally: title-block, highlight, embed-example, copy-code, equation-number, listing. Component layers come from `bundle.rs` loaders such as `load_listing_layer()` (`:417`), which reads `resources/scss/html/templates/quarto-listing.scss` via `TEMPLATES_RESOURCES`. To add `quarto-search.scss`: add the file there, add `load_search_layer()`, append in `compile.rs`, update `resources/scss/README.md`.

**bd-ulgr** (open, P1): "Design JS dependency handling…" covers vendor vs CDN, who declares dependencies, WASM delivery; its text lists search. Outline: `claude-notes/plans/2026-04-18-html-js-deps-design.md` (draft). The `js:*` artifact pattern has become the de facto answer while the strand stays open.

## 4. YAML config / schema layer

**No schema-validation layer for `website:` keys**; keys are read by typed resolvers. The `page-footer` chain end to end:
1. `quarto_config::resolve_website_value(meta, key)` (`crates/quarto-config/src/website.rs:39-48`) merges top-level `<key>` over `website.<key>`, top level winning.
2. `quarto_navigation::resolve_page_footer(meta) -> Option<PageFooter>` (`footer.rs:263-275`) handles `false`/`true`, then `PageFooter::from_config_value`.
3. `FooterGenerateTransform` (`transforms/footer_generate.rs:63+`) stores at `navigation.footer`.
4. `FooterRenderTransform` (`footer_render.rs`) writes `rendered.navigation.footer`.
5. Template slot renders it; preview `chromeSlots.tsx` injects the same HTML string.

**Simple flags:** `website_config.rs:86 website_llms_txt_enabled(meta)`; `transforms/config.rs:38 resolve_website_bool(meta, key, default)`.

**Diagnostics:** codes in `crates/quarto-error-catalog/error_catalog.json` (`{subsystem, title, message_template, docs_url, since_version}`, e.g. Q-13-3 navigation); docs page under `docs/errors/<subsystem>/`; attached with `DiagnosticMessageBuilder::…with_code("Q-…")` (e.g. `sidebar_generate.rs:198`). Precedent for "accepted but inert in this project type": the llms warning at `orchestrator.rs:429-443`.

## 5. WASM and hub-client constraint

- `wasm-quarto-hub-client/Cargo.toml` depends on `pampa`, `quarto-core` (full), `quarto-sass`, `quarto-highlight`, `quarto-lsp-core`, `quarto-pandoc-types`, `quarto-system-runtime` (js-bridge), others; leaves out `quarto-error-catalog` for bundle size. Bundle ~27 MB after `wasm-opt -Oz`; PWA precache limits in `hub-client/vite.config.ts`. New dependencies cost.
- `render_project_active_page_to_response` (`lib.rs:2333-2460`) runs `ProjectPipeline` with **`RenderMode::ActivePage`** (verified: `lib.rs:2432, 2454`; enum at `orchestrator.rs:905-910`: `Full | Subset(set) | ActivePage(path)`): Pass 1 over every file, `pre_render`, Pass 2 on the active page only, then `post_render`.
- Listings, navbar, sidebar run in WASM through `ProjectIndex`. Native-only `post_render` hooks (sitemap, L7, feeds, llms) do **not** run; no on-disk profile cache (`profile_cache.rs:19-22`).
- The default hub renderer is q2-preview (`RenderToPreviewAstRenderer`): returns AST JSON rendered by React, not HTML.

**Implication:** in the browser preview a search index can be built only from Pass-1 profiles (title, outline, derived description, keywords); full post-render text exists only for the active page. Any index builder in `quarto-core` must be pure Rust with no `scraper`, process spawning, or `std::fs` (I/O via `SystemRuntime`), or be cfg-gated out of wasm32. A Node or binary tool such as Pagefind's CLI cannot run there.

**Rules (`.claude/rules/wasm.md`):** never `#[cfg(any(target_arch = "wasm32", test))]`; async traits use `#[async_trait(?Send)]`; update `crates/pampa/tests/wasm_lua.rs` when touching the Lua WASM files.

**Hub-client FTS** (epic bd-mdet0yoy) is a different feature: editor search over project sources. Code: `hub-client/src/services/search/` (`SearchProvider` in `types.ts`; `inMemorySearchProvider.ts` uses MiniSearch 7.2 over raw source text; `snippet.ts`). Phase B (bd-b5ub29xd, deferred) plans "WASM plaintext + DocumentProfile boosting", the same per-document text extraction a site index needs; the extractor could be shared.

## 6. Plaintext extraction and heading anchors

- `pampa/src/writers/plaintext.rs`: `blocks_to_string(&[Block]) -> (String, Vec<DiagnosticMessage>)` at `:401`, `inlines_to_string` at `:389`. Limitation: **drops Table, DefinitionList, Custom nodes, note definitions, and non-plaintext RawBlocks with warnings** (`:300-330`). A search extractor wants table cell text and must suppress those warnings.
- Other helpers: `quarto_pandoc_types::inlines_to_plain_text` (`config_value.rs:24`), `ConfigValue::as_plain_text` (`:772`), private `append_block_text_skip_notes` in `listing_item_info.rs:263`. Six variants exist; bd-zzke tracks consolidating them.
- Per-section split at Finalization time: after `SectionizeTransform` (Normalization) each section is `Div{classes:["section","levelN"], id}` with the **id moved from the header to the section div** (`pampa/src/transforms/sectionize.rs:110-120`). `build_llms_view` in `llms.rs` already unwraps these and restores ids.
- **Heading ids:** derived in reader postprocess (`pampa/.../postprocess.rs:1101`) via `pampa::utils::autoid::auto_generated_id` (Pandoc-compatible, fallback `"section"`) with `dedup_scoped_heading_ids` (`autoid.rs:105,168`). `transforms/heading_id.rs` re-derives stale ids after shortcode and lmd expansion. Final ids are stable by Finalization; `profile.outline[].id` holds reader-time ids.

## 7. i18n

- Term files: `resources/language/_language.yml` plus 35 `_language-<lang>.yml`, embedded with `include_dir!` in `crates/quarto-core/src/language.rs:25`.
- **Search strings already exist** (`_language.yml:67-77`): `search-no-results-text`, `search-matching-documents-text`, `search-copy-link-title`, `search-hide-matches-text`, `search-more-match(es)-text`, `search-clear-button-title`, `search-text-placeholder`, `search-detached-cancel-button-title`, `search-submit-button-title`, `search-label`.
- `LanguageResolveStage` injects the resolved table at `meta.quarto.language`. Rust reads terms with `LanguageTerms::from_meta(&ast.meta)?.get("key")` (`language.rs:175`; listing example `listing_render.rs:245`). Templates read `$quarto.language.<key>$`. For a JS UI, serialize needed terms into the init script, as listings do for "No matching items".

## 8. Preview

- **`q2 preview`** (default) serves the embedded SPA plus the hub (`crates/quarto-preview/src/lib.rs`) and renders in WASM (ActivePage constraints). No `_site`, no post-render artifacts.
- **`q2 preview --static`** (`quarto-preview/src/static_mode/`) is a plain static-file router over `output_dir` (`server.rs`, any extension, `mime_guess`, `Cache-Control: no-store`). It injects an SSE reload client into HTML only, not `fetch()` requests. An index JSON or chunk files in `_site/` would just be served. `watch_policy.rs` ignores output files, so writing an index won't retrigger a render. Reload is SSE-driven by `crates/quarto/src/commands/preview_static.rs`. Subset re-renders need the index to merge with records from skipped pages (the llms manifest problem again). `no-store` means the browser re-fetches the index on each load.

## Takeaways for the design

1. **Model it on llms.txt.** A Finalization-tail `SearchCaptureTransform` writes path-less Project artifacts holding per-section records; a `write_search_index` step in `WebsiteProjectType::post_render` / `post_resources` emits the index. Persist per-page records under `.quarto/` so Subset renders can merge with skipped pages.
2. **Do not read rendered HTML back.** The post_render_upgrade header forbids reusing its machinery for search, and `scraper` is native-only.
3. **The WASM preview** can offer at most a profile-only index (title, outline, description, keywords) built in `post_render` from `ProjectIndex`, plus `?raw` JS injection in `entry.tsx`.
4. **Ship assets the existing way:** vendored JS in `resources/<feature>/`, a `js:search:*` artifact, `quarto-search.scss` as a quarto-sass layer, existing language terms, the navbar `search` flag that is already parsed, and new sidebar parsing (bd-6cme).
