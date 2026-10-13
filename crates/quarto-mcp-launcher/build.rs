//! Build script that locates the hub MCP server bundle at compile time.
//!
//! Mirrors `crates/quarto-preview/build.rs`. The `include_dir!` macro
//! needs a concrete compile-time path; this script resolves it:
//!
//! 1. If `ts-packages/quarto-hub-mcp/dist-bundle/index.mjs` exists,
//!    embed that directory — minus one file when the bundle declares
//!    the dedupe contract (see below).
//! 2. Otherwise embed a placeholder directory containing only a
//!    `BUNDLE_NOT_BUILT` marker, so the build still succeeds (fresh
//!    clones can `cargo build` before any npm step). `q2 mcp` then
//!    fails at runtime with an actionable message pointing at
//!    `cargo xtask build-hub-mcp-bundle`.
//!
//! WASM dedupe (plan
//! `claude-notes/plans/2026-10-13-wasm-embed-dedupe.md`): the ~36 MB
//! `wasm_quarto_hub_client_bg.wasm` rides the q2 binary exactly once,
//! via the `quarto-wasm-embed` crate. When the bundle's
//! `build-info.json` records `wasmSha256` (bundle.mjs's dedupe
//! marker), the WASM is stripped from this embed and the launcher
//! appends the shared bytes to the extraction set at runtime. A
//! pre-dedupe bundle (no `wasmSha256` field) keeps its own WASM in the
//! embed — self-consistent bytes for its own glue, just no dedupe,
//! until the next `cargo xtask build-hub-mcp-bundle`.
//!
//! The chosen path is exposed as `QUARTO_HUB_MCP_EMBED_DIR` via
//! `cargo:rustc-env`, consumed by `src/bundle.rs`.
//!
//! Stale-embed warning (see AGENTS.md and the 2026-05-20 preview-SPA
//! incident): a plain `cargo build` re-embeds whatever dist-bundle/
//! was last produced. The per-file rerun-if-changed entries below make
//! the *Rust* side rebuild when the bundle changes, but nothing makes
//! the bundle itself rebuild — run `cargo xtask build-hub-mcp-bundle`
//! (or `build-all`) after touching the TypeScript sources.

use std::path::{Path, PathBuf};

/// The WASM the dedupe strips from the embed (the launcher restores it
/// from `quarto-wasm-embed` at extraction time).
const WASM_FILENAME: &str = "wasm_quarto_hub_client_bg.wasm";

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.join("..").join("..");
    let real_bundle = workspace_root
        .join("ts-packages")
        .join("quarto-hub-mcp")
        .join("dist-bundle");

    let embed_dir = if real_bundle.join("index.mjs").is_file() {
        make_filtered_embed(&real_bundle)
    } else {
        make_placeholder()
    };

    println!(
        "cargo:rustc-env=QUARTO_HUB_MCP_EMBED_DIR={}",
        embed_dir.display()
    );

    // Re-run if the real bundle changes. Per-file entries catch
    // in-place content rewrites that a directory-mtime watch misses.
    println!("cargo:rerun-if-changed={}", real_bundle.display());
    if real_bundle.is_dir() {
        watch_recursive(&real_bundle);
    }
}

/// The filtered embed copy of `bundle` in `OUT_DIR`: every bundle file
/// except `wasm_quarto_hub_client_bg.wasm` when the bundle records
/// `wasmSha256` in its `build-info.json` (the dedupe marker bundle.mjs
/// writes). Rebuilt from scratch on every rerun so deleted/renamed
/// bundle files can never linger in the embed.
fn make_filtered_embed(bundle: &Path) -> PathBuf {
    let strip_wasm = bundle.join(WASM_FILENAME).is_file()
        && std::fs::read_to_string(bundle.join("build-info.json"))
            .is_ok_and(|info| info.contains("\"wasmSha256\""));

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let embed = out_dir.join("filtered-bundle");
    if embed.exists() {
        std::fs::remove_dir_all(&embed).expect("clear stale filtered bundle");
    }
    std::fs::create_dir_all(&embed).expect("create filtered bundle dir");
    copy_filtered(bundle, bundle, &embed, strip_wasm);
    embed
}

fn copy_filtered(root: &Path, dir: &Path, embed: &Path, strip_wasm: bool) {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            copy_filtered(root, &path, embed, strip_wasm);
            continue;
        }
        if strip_wasm && path.file_name().is_some_and(|n| n == WASM_FILENAME) {
            continue; // served from quarto-wasm-embed at extraction time
        }
        let rel = path.strip_prefix(root).expect("entry under walk root");
        let dest = embed.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).expect("create embed subdir");
        }
        std::fs::copy(&path, &dest).expect("copy bundle file");
    }
}

fn watch_recursive(root: &Path) {
    let entries = match std::fs::read_dir(root) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            watch_recursive(&path);
        }
    }
}

fn make_placeholder() -> PathBuf {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dir = out_dir.join("placeholder-bundle");
    std::fs::create_dir_all(&dir).expect("create placeholder bundle dir");
    let marker = dir.join("BUNDLE_NOT_BUILT");
    let contents = "Run `cargo xtask build-hub-mcp-bundle`, then rebuild the q2 binary.\n";
    let existing = std::fs::read_to_string(&marker).ok();
    if existing.as_deref() != Some(contents) {
        std::fs::write(&marker, contents).expect("write placeholder marker");
    }
    println!(
        "cargo:warning=ts-packages/quarto-hub-mcp/dist-bundle/ not found; `q2 mcp` will be \
         non-functional in this build. Run `cargo xtask build-hub-mcp-bundle` and rebuild."
    );
    dir
}
