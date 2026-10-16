/*
 * tests/integration/pandoc_read_stderr_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end regression test for GH #847 / bd-okfxemnj: `pandoc.read`
 * called from Lua must not leak the qmd reader's verbose trace to stderr.
 */

//! The qmd reader's `output_stream` is a `pampa -v` trace. The Lua
//! `pandoc.read` binding used to hand it `std::io::stderr()`, so a pipe
//! table parsed from an `{lmd}` cell printed one
//! `[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: -` line per
//! delimiter dash, unconditionally and with no diagnostic code.
//!
//! Only a spawned `q2 render` can observe this: the in-process
//! `readwrite.rs` unit tests cannot see what the binding writes to the
//! process's stderr.
//!
//! TDD note: written before the fix. On the unfixed tree the render
//! succeeds, both tables are correct, and stderr carries six trace lines.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// The issue's repro: a literal pipe table (never printed anything) and
/// the same table parsed through `pandoc.read` from an `{lmd}` cell.
const REPRO: &str = "---\n\
title: pandoc.read of a pipe table\n\
---\n\
\n\
| a | b |\n\
|---|---|\n\
| 1 | 2 |\n\
\n\
```{lmd}\n\
return pandoc.read(\"| a | b |\\n|---|---|\\n| 1 | 2 |\\n\", \"markdown\").blocks\n\
```\n";

fn run_q2_render(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    cmd.args(args);
    cmd.output().expect("spawn q2 binary")
}

#[test]
fn pandoc_read_of_a_pipe_table_leaves_stderr_clean() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    std::fs::write(dir.join("repro.qmd"), REPRO).unwrap();

    let output = run_q2_render(dir, &["repro.qmd", "--to", "html"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "render must succeed; stderr:\n{stderr}"
    );

    assert!(
        !stderr.contains("TOP-LEVEL MISSING NODE"),
        "the reader's verbose trace must not reach stderr through pandoc.read; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("Unhandled node kind"),
        "no node kind may be reported as unhandled; stderr:\n{stderr}"
    );

    // The fix must not change the output: both the literal table and the
    // pandoc.read one still render.
    let html = std::fs::read_to_string(dir.join("repro.html")).expect("repro.html is written");
    assert_eq!(
        html.matches("<table").count(),
        2,
        "expected two tables in the rendered HTML:\n{html}"
    );
}
