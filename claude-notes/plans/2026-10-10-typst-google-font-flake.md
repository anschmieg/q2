---
title: 'Typst Google-font download is a network flake in smoke-all: retry fetches and keep the diagnostic detail (bd-qvl8t3hy)'
date: 2026-10-10
description: 'A transient fonts.googleapis.com failure fails the whole smoke_all test and the harness drops the detail that would say why; make the fetch robust and keep the problem text.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-qvl8t3hy
---

**Branch:** `braid/bd-qvl8t3hy-typst-google-font-flake` in the main checkout (based on `main` @ `cbb088618`). No worktree, per the user's request.
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The strand was filed today with code pointers that all
check out at HEAD; the investigation confirms the analysis and adds three
findings that change the shape of the fix (HTTP status is never checked, the
fetch closure's `String` error cannot drive a retry policy, and the CSS is
refetched on every render regardless of cache state). The design questions
below are about policy, not about understanding.

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

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan.** Counting-closure test in `typst_google_fonts.rs`
  that fails N times then succeeds, asserting the retry count and that no
  Q-21-4 is emitted; a companion that fails every attempt and asserts exactly
  one Q-21-4 whose text includes the last error. A `quarto-test` unit test
  that a diagnostic with a `problem` produces a `LogMessage` containing both
  title and problem. If the status check lands in `native.rs`, a test against
  a local `std::net::TcpListener` serving a 503.
- **Phase 1 — Keep the problem text.** Both `runner.rs` sites emit
  `"{title}: {problem}"` (one line) when `problem` is `Some`. Factor the
  duplicated level mapping into the existing `diagnostics_to_messages` so the
  single-document path reuses it.
- **Phase 2 — Make the fetch robust.** Status check and timeout in the native
  `fetch_url` (or a narrower variant, per Q2/Q3); bounded retry with backoff
  around the CSS fetch and each `download` in `stage_google_font`.
- **Phase 3 — Fixture policy.** Apply the decision from Q4; if the fixtures
  stay under the default assertion, add nothing. If not, add the tolerance
  with a comment explaining the network dependence.
- **Phase 4 — Docs.** Note the retry/timeout behaviour in the Q-21-4 catalog
  entry if one exists in `crates/quarto-error-catalog`, and in the
  `typst_google_fonts.rs` module doc.

## Open design questions for the user

1. **Retry policy.** Uniform bounded retry (e.g. 3 attempts, ~250 ms then ~1 s
   backoff) on every fetch `Err`, accepting the offline-latency cost from
   finding 2? Or widen the closure's error type so DNS/connection-refused
   fail fast and only timeouts, resets, and 5xx retry? The first is a
   one-file change; the second touches the `pandoc_write.rs` closure and the
   `RuntimeError` mapping.
2. **HTTP status handling.** Make the native `fetch_url` treat non-2xx as an
   error for every caller (Lua mediabag, user filters, listing substitution,
   the Typst font path)? That is the correct behaviour and matches the
   `quarto-source-fetch` precedent, but it changes what Lua filters see on a
   404. The alternative is a status check only in the Google-font path, which
   needs a richer return type from the closure.
3. **Timeout.** Put a 30 s request timeout on the native `fetch_url` (matching
   `quarto-source-fetch`), applying to all callers? Or keep `fetch_url`
   unchanged and only time out the font fetch? A reqwest `blocking::Client`
   with `.timeout()` replaces the bare `blocking::get` either way.
4. **Fixture policy.** With retry plus timeout plus status check in place, keep
   the five fixtures under the default `noErrorsOrWarnings` so a real
   regression in the Google-font feature still reddens CI? (Recommended.) Or
   make the network dependence explicit per fixture, accepting that a broken
   CSS parser would then pass silently?
5. **Message shape.** `"{title}: {problem}"` on one line (recommended: greppable
   in CI logs, friendly to `printsMessage` regexes), or the full
   `DiagnosticMessage::to_text` rendering with hints on multiple lines?
6. **CSS caching (finding 3).** Settled 2026-10-10: out of scope here. Filed
   as bd-f48l642w (discovered-from this strand) for a standalone fix.

## Risks / tradeoffs (draft)

- Changing `fetch_url` globally (Q2, Q3) is the right fix but widens the blast
  radius to Lua filters and listing substitution. Those callers currently
  receive HTML error pages as success; a filter that happened to tolerate that
  would start seeing an error instead. Grep found no test relying on it.
- Retry adds latency to failure paths only, but failure paths include "user is
  on a plane". Four families times three attempts times backoff is noticeable.
  Q1 decides whether that is acceptable.
- None of this removes the network dependency from CI; it narrows the window
  in which a blip fails the run. If Google is down for a minute, smoke-all
  still fails, now with a message that says so.
- `typst_google_fonts.rs` is unit-tested through its closure, so the retry is
  testable without a network. The `native.rs` change is not covered by any
  existing test; Phase 0 proposes a loopback server for it.
