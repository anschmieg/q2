# bd-lt7rgi90 repro: `pandoc.utils.stringify(pandoc.Nodes(...))` is `""`

Run from the repo root (debug build of the `pampa` binary):

```bash
cargo build -p pampa --features terminal-support
target/debug/pampa -F claude-notes/plans/stringify-nodes-investigation/filter.lua \
  claude-notes/plans/stringify-nodes-investigation/doc.qmd -t json > /dev/null
```

The filter prints one line per case to stderr. The Nodes values are built
from strings and elements so this runs on `main` without PR #859 (on
`main`, `pandoc.Nodes(42)` still errors with "got number").

## Output at `main` @ 8e0345009 (before the fix)

```
Nodes('42')                        -> ""
Nodes({'a','2'})                   -> ""
Nodes(pandoc.Para{'p'})            -> ""
Nodes()  (empty)                   -> ""
Span(Nodes('42')) [workaround]     -> "42"
type(Nodes('x'))                   -> Nodes
blocks_to_inlines(Nodes(Para))     -> 0 inlines
equals(Nodes('x'), Nodes('x'))     -> false
```

## Output after the fix (same command, same branch)

```
Nodes('42')                        -> "42"
Nodes({'a','2'})                   -> "a2"
Nodes(pandoc.Para{'p'})            -> "p"
Nodes()  (empty)                   -> ""
Span(Nodes('42')) [workaround]     -> "42"
type(Nodes('x'))                   -> Nodes
blocks_to_inlines(Nodes(Para))     -> 1 inlines
equals(Nodes('x'), Nodes('x'))     -> true
```
