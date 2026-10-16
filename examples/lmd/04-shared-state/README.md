# Shared state between lmd cells

Demonstrates a setup cell that defines a variable and a function, used by inline cells later in the document, including one in a heading.

This example backs the lmd page of the Quarto 2 documentation
(`docs/guides/authoring/lmd.qmd`).

## Run it

From the repository root:

```bash
cargo run --bin q2 -- render examples/lmd/04-shared-state
```

The output is `document.html` next to `document.qmd`.
