# Investigation artifacts for bd-okfxemnj (GH #847)

Captured 2026-10-15 at `main` @ `e308835bd` with a debug build of `q2` and `pampa`.

## `repro.qmd` — the issue's repro

```sh
cargo run --bin q2 -- render claude-notes/plans/pandoc-read-delimiter-dash-noise-investigation/repro.qmd --to html
```

stderr (6 lines, one per `-` in `|---|---|`):

```
Rendering single file: .../repro.qmd
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
```

`repro.html` contains two correct `<table>` elements. The literal pipe table
in the body contributes no lines; only the `pandoc.read` call does.

With a one-line experimental change in `crates/pampa/src/lua/readwrite.rs`
(`std::io::stderr()` → `std::io::sink()` as the reader's buffer) the same
render prints only the `Rendering single file:` line and still emits both
tables. (Change reverted; it is the proposed Phase 1.)

## `probe.qmd` — which other node kinds reach the catch-all

The `pampa` CLI writes the same buffer to stderr only under `-v`:

```sh
cargo run --bin pampa -- -v claude-notes/plans/pandoc-read-delimiter-dash-noise-investigation/probe.qmd -t native 2>&1 >/dev/null \
  | grep 'TOP-LEVEL' | sort | uniq -c
```

```
   5 [TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -
   2 [TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: pipe_table_align_left
   2 [TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: pipe_table_align_right
   1 [TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: ![
```

Without `-v` the same run prints nothing from the catch-all (only the real
Q-2-39 / Q-2-54 diagnostics the probe deliberately triggers). So the full set
of token kinds that reach the catch-all on ordinary input is: `-`,
`pipe_table_align_left`, `pipe_table_align_right` (all children of
`pipe_table_delimiter_cell`) and `![` (child of `pandoc_image`). Bullet
lists, ordered lists, task lists, block quotes, footnotes, fenced divs,
shortcodes, math, and the `|` cell separators do not reach it.

## `hyphens.qmd` — does any other construct emit a bare `-` token?

Run on the unfixed tree (Phase 2 arms stashed) to check the scope of a
top-level `"-"` arm:

```sh
cargo run --bin pampa -- -v claude-notes/plans/pandoc-read-delimiter-dash-noise-investigation/hyphens.qmd -t native 2>&1 >/dev/null \
  | grep 'TOP-LEVEL' | sort | uniq -c
```

```
   1 [TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: ![
```

Hyphens in running text, `a - b`, `--`, link destinations and titles
(`[a-b](c-d "t-u")`), autolinks, code spans, math, list markers and
thematic breaks produce no bare `-` node. The grammar's
`punctuation_without` helper (`common/common.js`) does list `-` among the
anonymous punctuation tokens allowed in a link destination, but those are
folded into the destination's token before the visitor sees them. So the
delimiter row is the only source of `-` nodes reaching `native_visitor`.
