---
title: 'Typst Google-font download is a network flake in smoke-all: retry fetches and keep the diagnostic detail (bd-qvl8t3hy)'
date: 2026-10-10
description: 'A transient fonts.googleapis.com failure fails the whole smoke_all test and the harness drops the detail that would say why; make the fetch robust and keep the problem text.'
status: approved  # Design questions answered 2026-10-10; ready to implement
braid:
  strand: bd-qvl8t3hy
---

**Branch:** `braid/bd-qvl8t3hy-typst-google-font-flake` in the main checkout (based on `main` @ `cbb088618`). No worktree, per the user's request.
**Design settled 2026-10-10.** The six questions in the original skeleton are
answered in § Decisions below; the phases are concrete.

## Triage verdict

**Ready to implement.** The strand was filed today with code pointers that all
check out at HEAD; the investigation confirms the analysis and adds three
findings that change the shape of the fix (HTTP status is never checked, the
fetch closure's `String` error cannot drive a retry policy, and the CSS is
refetched on every render regardless of cache state). The policy questions
those raised were answered the same day; see § Decisions.

## Issue context

Status open, priority 2, type bug, labels `ci` + `typst`. Created 2026-10-10 by
Carlos Scheidegger, so nothing in it is stale.

PR #818's macOS Test Suite run failed on one smoke-all fixture,
`typst/brand-yaml/typography/kitchen-sink-1/brand-typography.qmd`, with a
single `[WARN] Google font not available to Typst`. Ubuntu passed in the same
run and `main` passed at the same hour. The PR touched only TypeScript and
hub-client code. First occurrence since `aa0d7c141` (2026-09-30) made every
Typst render of a `source: google` brand fetch the font CSS and font files at
render time.

The strand asks for four things: keep the Q-21-4 problem text in the harness
output, retry transient fetch failures, consider a timeout on the native
`fetch_url`, and decide whether the five network-dependent fixtures stay under
the default `noErrorsOrWarnings` assertion.

## Dependency graph

The graph is a single edge.

- **discovered-from**: bd-c72wsugj (in progress) — the October flaky-tests
  pass. That strand is about a vitest timing flake in
  `quarto-sync-client/doc-inventory.test.ts`; the Google-font flake surfaced
  while watching PR #818's CI and was explicitly kept out of that PR's scope.
  Nothing there constrains this design. The shared motivation is the one
  stated in bd-c72wsugj: a known-flaky gate trains people to re-run reds
  without reading them.
- **blocks**: none. No incoming pressure beyond CI hygiene.
- **related**: none recorded.

## What the code looks like today

Every path in the strand resolves at HEAD and the line numbers are current.

**The fetch chain.** `crates/quarto-core/src/stage/stages/pandoc_write.rs:334`
`stage_typst_brand_fonts` (native only, `#[cfg(not(target_arch = "wasm32"))]`)
wraps `runtime.fetch_url` in a `Fn(&str) -> Result<Vec<u8>, String>` closure
and hands it to `crates/quarto-core/src/typst_google_fonts.rs:250`
`stage_brand_fonts`. `stage_google_font` (line 274) fetches the CSS, parses the
`@font-face` rules, and calls `download` (line 339) for each file URL that is
not already in `.quarto/typst/fonts/` under the project. Any `Err` from the
closure becomes a Q-21-4 warning via `unavailable` (line 235), whose
`problem` text names the family, the URL, and the underlying error string.

**The native fetch.** `crates/quarto-system-runtime/src/native.rs:270`
`fetch_url` is a bare `reqwest::blocking::get(url)` followed by
`response.bytes()`. No timeout, no retry, and (a finding not in the strand) **no
status check**: a 4xx/5xx response is returned as `Ok` with the HTML error body.
The reqwest dependency is `0.13` with `rustls` + `blocking` features only.

**The harness.** `crates/quarto-test/src/runner.rs:574` `diagnostics_to_messages`
and the single-document path at `:661` both build `LogMessage { level, message:
diag.title.clone() }`, dropping `diag.problem`. `NoErrorsOrWarnings` then prints
`[WARN] <title>` and nothing else. The default `noErrorsOrWarnings` is applied at
`:420` to any fixture that declares none of `noErrors` / `noErrorsOrWarnings` /
`shouldError`.

**Assertion audit** (strand work item 1 asked for this). `printsMessage`
matches with `regex.is_match`, i.e. unanchored substring, so appending problem
text to the message cannot break an existing matcher. No smoke-all fixture
mentions `Q-21-4` or `Google font`; the `printsMessage` users under `typst/`
are the `font-paths/*`, `orange-book`, and `orange-book-margin` fixtures, all
matching unrelated text. No `#[test]` outside `typst_google_fonts.rs`
references Q-21-4.

**The five fixtures** all exist and are the complete set of `source: google`
users under `crates/quarto/tests/smoke-all/` (confirmed by grep):
`typst/brand-yaml/typography/{kitchen-sink-1,kitchen-sink-2}/_brand.yml`,
`typst/brand-yaml/typography/{font-list,title-inherit-base-family}.qmd`, and
`typst/syntax-highlighting/brand-monospace-inheritance/_brand.yml`.

**CI.** `test-suite.yml` caches Rust artifacts only. `.quarto/` under smoke-all
is gitignored and not cached, so every CI run fetches every font cold. The
hub-client E2E smoke-all run is unaffected: the WASM build skips
`stage_typst_brand_fonts` entirely.

**Reproducibility.** The flake is a network event; it is not reproducible on
demand. The `stage_warns_instead_of_failing_when_offline` unit test already
shows the deterministic half (one `Err` from the closure yields one Q-21-4),
and the CI log in the strand shows the harness half (title only). No repro
artifact was captured.

### Findings beyond the strand's analysis

1. **HTTP status is never checked.** A 5xx from `fonts.googleapis.com` comes
   back as `Ok(html)`. `parse_font_faces` finds no `@font-face` rules, and the
   user gets "Google Fonts returned no font files for it (is the family name
   spelled correctly?)". That is a misdiagnosis, and a retry keyed on `Err`
   would never fire for it. Whatever retry lands needs a non-2xx to be an
   error first. The same gap applies to the other `fetch_url` callers (Lua
   `mediabag`, `user_filters`, listing `substitute`, `render.rs`).
2. **The closure's error type is `String`.** A retry inside
   `typst_google_fonts.rs` cannot tell "DNS says the host does not exist" from
   "connection reset", so it must either retry everything uniformly or the
   closure signature must carry a classification. Uniform retry means an
   offline user pays the full retry budget once per family (kitchen-sink-1 has
   four) on every Typst render.
3. **The CSS is refetched on every render.** The existing test
   `stage_downloads_once_and_reuses_cache` pins this: a warm file cache still
   costs one CSS request per family. So even a developer with a populated
   `.quarto/typst/fonts/` has an irreducible network dependency per render.
   This does not affect CI (always cold) but it is the reason the feature
   cannot be made offline-safe by caching alone. Filed separately as
   bd-f48l642w.
4. **Precedent in the tree.** `crates/quarto-source-fetch/src/fetch.rs` uses
   `ureq` with `timeout_global(limits.request_timeout)` (30 s default in
   `limits.rs`), explicit 2xx handling, and a comment explaining why a 404 body
   must not be written to disk. The native runtime's `fetch_url` predates that
   and has none of it. Both the timeout value and the status-handling comment
   are worth borrowing.

## Decisions (2026-10-10)

Answers to the skeleton's six design questions, recorded here so the phases
below need no further alignment.

1. **Retry policy: uniform, one file.** Bounded retry on every fetch `Err`
   inside `typst_google_fonts.rs`, 3 attempts with roughly 250 ms then 1 s
   backoff. The closure keeps its `Fn(&str) -> Result<Vec<u8>, String>`
   signature. The offline-latency cost from finding 2 is accepted for now; a
   classified error type can come later if it bites.
2. **HTTP status: fix it for every caller.** The native `fetch_url` treats a
   non-2xx response as an error. Lua filters that relied on receiving a 404
   body as success will need to adjust; that is acceptable.
3. **Timeout: 30 s on the native `fetch_url`, all callers.** Matches the
   `quarto-source-fetch` default. Adjust later if a caller needs more.
4. **Fixture policy: keep the default.** The five network-dependent fixtures
   stay under the implicit `noErrorsOrWarnings`, so a real regression in the
   Google-font feature still reddens CI. No fixture edits.
5. **Message shape: the full diagnostic rendering.** The harness's
   `LogMessage.message` becomes `diag.to_text(None)` rendered with
   `TextRenderOptions::plain()` (no ANSI, no OSC 8), so the CI log shows the
   title, the problem, and the hints exactly as a user would see them.
   Scope note: `LogMessage` is private to `quarto-test`, and the harness calls
   `render_to_file` directly rather than the CLI. The CLI's `--json-errors`
   and `--strict` paths in `crates/quarto/src/commands/render.rs` consume the
   `DiagnosticMessage` itself and are untouched by this change, so JSON output
   stays pure JSON and strict promotion keeps working without additional work.
6. **CSS caching: separate strand.** Filed as bd-f48l642w.

## Phases

Work proceeds test-first within each area (harness, native fetch, retry)
rather than writing every test before any implementation, so each phase
leaves the tree green. Checkboxes are updated as work lands.

### Phase 1 — Harness keeps the diagnostic detail (`crates/quarto-test`)

- [x] Test: a warning with a `problem` and a hint yields a `LogMessage` whose
      text contains title, problem, and hint, with no escape sequences, via
      `diagnostics_to_messages`.
- [x] Test: the single-document render path produces the same shape.
- [x] Replace `diag.title.clone()` at both `runner.rs` sites with
      `diag.to_text_with_options(None, &TextRenderOptions::plain())`.
- [x] Fold the single-document path's duplicated level mapping into
      `diagnostics_to_messages` (one place builds a `LogMessage`).
- [x] Continuation lines of multi-line messages are indented in the
      `noErrors` / `noErrorsOrWarnings` / `printsMessage` failure output
      (`LogMessage::indented`), so a rendered diagnostic stays attached to
      its `[LEVEL]` prefix.
- [x] `cargo nextest run -p quarto-test` green.
- [x] Smoke-all `printsMessage` fixtures (`typst/orange-book*`,
      `typst/font-paths/*`) and the Google-font fixtures still pass under the
      harness change (run with `SMOKE_FILTER`, network up).

### Phase 2 — Native `fetch_url` (`crates/quarto-system-runtime/src/native.rs`)

- [x] Test: loopback `TcpListener` answering `503` makes `fetch_url` return
      `Err` naming the status and the URL.
- [x] Test: loopback listener that accepts and never writes makes the fetch
      return `Err` within a short, injected timeout (200 ms against a 1.5 s
      stall).
- [x] Test: 200 response round-trips body and `content-type`.
- [x] One shared `reqwest::blocking::Client` (`OnceLock`) built with the 30 s
      `FETCH_URL_TIMEOUT`; `fetch_blocking(url, timeout)` applies a
      per-request timeout on top, which is what the tests inject.
- [x] Non-2xx is `Err` (explicit `status.is_success()` check rather than
      `error_for_status()`, so the message reads `HTTP 503 Service
      Unavailable for <url>`).
- [x] Failures map to `RuntimeError::Network`, not `NotSupported`.
- [x] Comment (borrowed from `quarto-source-fetch`) on why a non-2xx body is
      not the answer.
- [x] Bonus, same class of bug: the native `fetch_url_hardened` now honors its
      `FetchPolicy::timeout_ms` (15 s default). The trait default delegated to
      `fetch_url`, so the policy's timeout was never enforced natively; the
      remote-image prefetch stage is the caller.
- [x] Audit callers: `pampa` mediabag maps `Err` to a logged nil (Lua now
      sees nil instead of an HTML body on 404); `user_filters`, listing
      `substitute`, `render.rs`, and the `pandoc_svg_rasterize` test wrapper
      delegate unchanged; `prefetch_remote_images` already accepts both
      `Network` and `NotSupported`.
- [x] `cargo nextest run -p quarto-system-runtime` green (136 tests); the
      fetch-related quarto-core and pampa tests pass (77).

### Phase 3 — Retry in `stage_google_font` (`crates/quarto-core/src/typst_google_fonts.rs`)

- [ ] Test: closure fails twice then succeeds: three calls, no Q-21-4, file
      cached.
- [ ] Test: closure always fails: exactly one Q-21-4 per family, text names
      the final error and the attempt count.
- [ ] Test: existing `stage_downloads_once_and_reuses_cache` still asserts
      3 calls on the success path (no retries fired).
- [ ] `fetch_with_retry` helper with an injectable sleep; 3 attempts, 250 ms
      then 1 s between them.
- [ ] Use it for the CSS fetch and inside `download`.
- [ ] Q-21-4 problem text reports the final error and attempt count.
- [ ] Module doc: fetch retries; native runtime times out at 30 s.
- [ ] `cargo nextest run -p quarto-core typst_google_fonts` green.

### Phase 4 — Verification

- [ ] `cargo nextest run -p quarto --test integration smoke_all` with the
      network up: the five Google-font fixtures pass.
- [ ] `cargo xtask verify --skip-hub-build` green.

### Phase 5 — Docs

- [ ] Q-21-4 entry in `crates/quarto-error-catalog`, if one exists: mention
      retry and timeout.
- [ ] PR description notes the Lua-visible change (non-2xx now fails
      `mediabag.fetch`).

## Risks / tradeoffs

- Changing `fetch_url` globally (decisions 2 and 3) is the right fix but widens the blast
  radius to Lua filters and listing substitution. Those callers currently
  receive HTML error pages as success; a filter that happened to tolerate that
  would start seeing an error instead. Grep found no test relying on it.
- Retry adds latency to failure paths only, but failure paths include "user is
  on a plane". Four families times three attempts times backoff is noticeable.
  Decision 1 accepts that for now.
- None of this removes the network dependency from CI; it narrows the window
  in which a blip fails the run. If Google is down for a minute, smoke-all
  still fails, now with a message that says so.
- `typst_google_fonts.rs` is unit-tested through its closure, so the retry is
  testable without a network. The `native.rs` change is not covered by any
  existing test; Phase 0 proposes a loopback server for it.
