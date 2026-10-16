# Reading metadata from lmd

Demonstrates reading values with `meta` and `quarto.metadata.get`, then computing on them: counting, joining, string methods, a fallback for a missing key.

This example backs the lmd page of the Quarto 2 documentation
(`docs/guides/authoring/lmd.qmd`).

## Run it

From the repository root:

```bash
cargo run --bin q2 -- render examples/lmd/02-metadata
```

The output is `document.html` next to `document.qmd`.
