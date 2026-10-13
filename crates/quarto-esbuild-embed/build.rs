//! Locate the esbuild binary for the target and expose it to `include_bytes!`.
//!
//! `cargo xtask fetch-esbuild --target <triple>` writes
//! `ts-packages/esbuild-embed/<triple>/esbuild[.exe]` (gitignored). When it is
//! absent this build embeds an empty placeholder: the crate then reports
//! "nothing embedded" and `q2 call build-ts-extension` falls back to
//! `ESBUILD_BINARY_PATH` or the workspace's `node_modules/@esbuild`.

use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").expect("TARGET");
    let exe = if target.contains("windows") {
        "esbuild.exe"
    } else {
        "esbuild"
    };
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let source = manifest
        .join("../../ts-packages/esbuild-embed")
        .join(&target)
        .join(exe);
    println!("cargo:rerun-if-changed={}", source.display());

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("esbuild.bin");
    if source.is_file() {
        std::fs::copy(&source, &out).expect("copy esbuild binary into OUT_DIR");
    } else {
        std::fs::write(&out, []).expect("write placeholder");
        println!(
            "cargo:warning=quarto-esbuild-embed: {} not found; embedding a placeholder. \
             Run `cargo xtask fetch-esbuild --target {target}` and rebuild to embed esbuild.",
            source.display()
        );
    }
}
