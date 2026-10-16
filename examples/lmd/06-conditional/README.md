# Conditional content

Demonstrates a callout that appears only when a front-matter flag is set, and a per-format branch with `quarto.doc.is_format`.

This example backs the lmd page of the Quarto 2 documentation
(`docs/guides/authoring/lmd.qmd`).

## Run it

From the repository root:

```bash
cargo run --bin q2 -- render examples/lmd/06-conditional
```

The output is `document.html` next to `document.qmd`.
