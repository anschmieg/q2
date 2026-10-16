# A filter on a fragment

Demonstrates `Blocks:walk` applied to a fragment: pandoc's filter vocabulary on a value in hand.

This example backs the lmd page of the Quarto 2 documentation
(`docs/guides/authoring/lmd.qmd`).

## Run it

From the repository root:

```bash
cargo run --bin q2 -- render examples/lmd/07-walk
```

The output is `document.html` next to `document.qmd`.
