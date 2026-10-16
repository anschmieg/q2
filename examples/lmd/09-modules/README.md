# Reusable modules

Demonstrates `require` of a module written in lmd: `components/cards.lmd`
is a Lua chunk with markdown fragments that returns a table of functions,
and two cells in `document.qmd` load it and build cards from it.

This example backs the lmd page of the Quarto 2 documentation
(`docs/guides/authoring/lmd.qmd`).

## Run it

From the repository root:

```bash
cargo run --bin q2 -- render examples/lmd/09-modules
```

The output is `document.html` next to `document.qmd`.
