# Echoing the desugared Lua

Demonstrates `--| echo: true`, which shows the Lua a cell actually ran above its result.

This example backs the lmd page of the Quarto 2 documentation
(`docs/guides/authoring/lmd.qmd`).

## Run it

From the repository root:

```bash
cargo run --bin q2 -- render examples/lmd/08-echo
```

The output is `document.html` next to `document.qmd`.
