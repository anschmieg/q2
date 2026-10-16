# Fragments as functions

Demonstrates a `card(title, body)` function returning a fragment, with a block cell nested inside the fragment to splice the body.

This example backs the lmd page of the Quarto 2 documentation
(`docs/guides/authoring/lmd.qmd`).

## Run it

From the repository root:

```bash
cargo run --bin q2 -- render examples/lmd/05-fragment-functions
```

The output is `document.html` next to `document.qmd`.
