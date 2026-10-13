//! `cargo xtask vendor-react` — rebuild the vendored React ES modules
//! under `resources/js/react/` (bd-react-components-a8flgldm).
//!
//! Runs `resources/js/react/build.mjs` (esbuild over the workspace's
//! `node_modules/react` and `react-dom`, the versions hub-client pins)
//! and then `check.mjs`, which links the four files the way a browser
//! import map would and renders a stateful component under jsdom. The
//! files are committed; `components/runtime.rs` embeds them with
//! `include_bytes!` and pins their versions against `build-info.json`,
//! so a React bump that skips this command fails the Rust test suite.
//!
//! Needs the repo's `npm install` to have run (esbuild and jsdom come from
//! the workspace `node_modules`).

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use crate::util::nested_command;

pub fn run() -> Result<()> {
    let project_root = find_project_root()?;
    let dir = project_root.join("resources/js/react");
    if !dir.join("build.mjs").is_file() {
        bail!(
            "resources/js/react/build.mjs not found under {}",
            project_root.display()
        );
    }

    println!("━━━ Vendoring React ES modules ━━━");
    for script in ["build.mjs", "check.mjs"] {
        let status = nested_command("node")
            .arg(dir.join(script))
            .current_dir(&project_root)
            .status()
            .with_context(|| format!("Failed to spawn node for resources/js/react/{script}"))?;
        if !status.success() {
            bail!("resources/js/react/{script} failed");
        }
    }
    println!("✓ resources/js/react/ is up to date");
    Ok(())
}

fn find_project_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("Failed to get current directory")?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = std::fs::read_to_string(&cargo_toml)?;
            if content.contains("[workspace]") {
                return Ok(dir);
            }
        }
        if !dir.pop() {
            bail!("Could not find workspace root (Cargo.toml with [workspace])");
        }
    }
}
