/*
 * build_ts_extension_e2e.rs
 *
 * P2-18 — end-to-end test for `q2 call build-ts-extension`.
 *
 * Invokes `q2 call build-ts-extension` on a temp COPY of the echo-engine fixture,
 * then verifies that the output `.js` bundle is produced, non-empty, and
 * contains a recognisable token from the source.
 *
 * The build spawns the esbuild binary from the workspace's
 * `node_modules/@esbuild/<platform>` (installed by `npm ci`); there is no
 * skip gate, so a missing binary fails with the "run npm ci" message.
 *
 * P2-18 non-vacuity: the fixture is copied to a tempdir and its `dist/`
 * output is DELETED before the build, so the committed `dist/echo-engine.js`
 * is never read or mutated. The assertion therefore proves the build ACTUALLY
 * produced the `.js` this run — if `build-ts-extension` skipped the esbuild
 * step, no `.js` exists in the tempdir and the test goes RED. (The
 * earlier form asserted the COMMITTED `dist/echo-engine.js` existed, which is
 * always true regardless of whether the build ran — a vacuous pass.)
 */

use std::path::Path;
use std::process::Command;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Recursively copy `src` into `dst` (dst is created).
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .and_then(|p| p.parent()) // repo root
        .expect("Failed to find workspace root from CARGO_MANIFEST_DIR")
        .to_path_buf()
}

/// The esbuild binary `npm ci` installs for this platform.
fn workspace_esbuild() -> std::path::PathBuf {
    let (pkg, bin) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("darwin-arm64", "bin/esbuild"),
        ("macos", "x86_64") => ("darwin-x64", "bin/esbuild"),
        ("linux", "x86_64") => ("linux-x64", "bin/esbuild"),
        ("linux", "aarch64") => ("linux-arm64", "bin/esbuild"),
        ("windows", "x86_64") => ("win32-x64", "esbuild.exe"),
        (os, arch) => panic!("no esbuild package for {os}/{arch}"),
    };
    workspace_root()
        .join("node_modules/@esbuild")
        .join(pkg)
        .join(bin)
}

/// Write a minimal extension under `dir` whose entry source is `source`.
fn write_extension(dir: &Path, source: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("_extension.yml"),
        "title: T\nversion: 1.0.0\ncontributes:\n  engines:\n    - path: dist/ext.js\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/ext.ts"), source).unwrap();
}

/// P2-18: `q2 call build-ts-extension` produces a loadable `.js` bundle.
///
/// Runs the command on the committed `echo-engine` fixture under
/// `crates/quarto-core/tests/fixtures/extensions/echo-engine/`, copied to a
/// tempdir. The fixture's `_extension.yml` declares the output path as
/// `dist/echo-engine.js`, which — after deleting any pre-existing copy in the
/// tempdir — we assert exists, is non-empty, and was produced by this build.
#[test]
fn build_ts_extension_produces_bundle() {
    // The fixture is committed relative to the quarto crate root.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    // quarto crate is at crates/quarto/, so workspace root is ../..
    let workspace_root = Path::new(manifest_dir)
        .parent() // crates/
        .and_then(|p| p.parent()) // repo root
        .expect("Failed to find workspace root from CARGO_MANIFEST_DIR");

    let committed_ext =
        workspace_root.join("crates/quarto-core/tests/fixtures/extensions/echo-engine");
    assert!(
        committed_ext.exists(),
        "fixture directory missing: {}",
        committed_ext.display()
    );

    // P2-18 non-vacuity: copy the fixture to a temp dir and delete its committed
    // `dist/` so the build MUST produce the `.js` fresh (and the committed
    // `dist/echo-engine.js` in the working tree is never touched). The temp copy
    // lives under the workspace's gitignored `target/` so that build-ts-extension's
    // workspace auto-detection (walking up for `ts-packages/quarto-api`) still
    // finds the workspace aliases and `node_modules/@esbuild` — a copy fully
    // outside the workspace only gets the shipped (alias-free) tier.
    let tmp = tempfile::TempDir::new_in(workspace_root.join("target")).expect("tempdir");
    let ext_dir = tmp.path().join("echo-engine");
    copy_dir(&committed_ext, &ext_dir);
    let js_path = ext_dir.join("dist/echo-engine.js");
    if js_path.exists() {
        std::fs::remove_file(&js_path).expect("remove pre-existing dist bundle in temp copy");
    }
    assert!(
        !js_path.exists(),
        "precondition: dist bundle must not exist before the build"
    );

    // Run q2 call build-ts-extension on the temp copy.
    let output = Command::new(Q2_BIN)
        .args(["call", "build-ts-extension"])
        .arg(&ext_dir)
        .current_dir(workspace_root)
        .output()
        .expect("Failed to spawn q2");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "q2 call build-ts-extension failed.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Assert the declared output path was produced BY THIS BUILD (it did not
    // exist a moment ago), proving the esbuild step actually ran.
    assert!(
        js_path.exists(),
        "Expected bundle at {} but it was not created.\nstdout: {stdout}\nstderr: {stderr}",
        js_path.display()
    );

    let js_content = std::fs::read_to_string(&js_path).expect("Failed to read bundle output");

    // Non-empty.
    assert!(
        !js_content.is_empty(),
        "Bundle at {} is empty",
        js_path.display()
    );

    // Sanity-check: the bundle must contain the engine name token.
    // Named revert: if the bundler output is empty or doesn't bundle the source,
    // this check goes RED.
    assert!(
        js_content.contains("echo"),
        "Bundle does not contain 'echo' token — bundling may have failed:\n{}",
        &js_content[..js_content.len().min(500)]
    );

    // The bundle should be a valid ES module (has 'export').
    assert!(
        js_content.contains("export"),
        "Bundle does not contain 'export' — may not be a valid ES module:\n{}",
        &js_content[..js_content.len().min(500)]
    );
}

/// Missing-test #1 (seam T2): the top-level `q2 build-ts-extension` command
/// (without `call`) must no longer exist — Q1 parity moved it under
/// `q2 call build-ts-extension`. Does not require `deno` (clap rejects the
/// subcommand before dispatch ever runs).
///
/// Named revert: re-adding the top-level
/// `#[command(name = "build-ts-extension")] BuildTsExtension { .. }` variant
/// to `Commands` in main.rs makes this command parse again → RED.
#[test]
fn top_level_build_ts_extension_removed() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("Failed to find workspace root from CARGO_MANIFEST_DIR");

    let output = Command::new(Q2_BIN)
        .arg("build-ts-extension")
        .arg(".")
        .current_dir(workspace_root)
        .output()
        .expect("Failed to spawn q2");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "q2 build-ts-extension (no `call`) unexpectedly succeeded — the \
        top-level command should have been removed.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("unrecognized subcommand") || stderr.contains("error:"),
        "expected clap to reject the removed top-level subcommand; stderr: {stderr}"
    );
}

/// Two builds of the same extension are byte-identical (the fixed relative
/// entry keeps esbuild's path comments stable).
#[test]
fn build_ts_extension_is_byte_stable() {
    let tmp = tempfile::TempDir::new_in(workspace_root().join("target")).expect("tempdir");
    let ext = tmp.path().join("stable");
    copy_dir(
        &workspace_root().join("crates/quarto-core/tests/fixtures/extensions/echo-engine"),
        &ext,
    );
    let build = || {
        let out = Command::new(Q2_BIN)
            .args(["call", "build-ts-extension"])
            .arg(&ext)
            .output()
            .expect("spawn q2");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::read(ext.join("dist/echo-engine.js")).unwrap()
    };
    assert_eq!(build(), build());
}

/// A Deno-only import fails with a message naming the `node:` replacement.
#[test]
fn deno_only_import_fails_with_a_node_hint() {
    let tmp = tempfile::TempDir::new_in(workspace_root().join("target")).expect("tempdir");
    let ext = tmp.path().join("ext");
    write_extension(
        &ext,
        "import { join } from \"jsr:@std/path\";\nexport default join;\n",
    );
    let out = Command::new(Q2_BIN)
        .args(["call", "build-ts-extension"])
        .arg(&ext)
        .output()
        .expect("spawn q2");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("jsr:@std/path"),
        "esbuild error missing: {stderr}"
    );
    assert!(stderr.contains("node:"), "hint missing: {stderr}");
}

/// Outside a q2 workspace (the installed-binary case) type-only imports of
/// `@quarto/*` build, and value imports fail naming the workspace.
#[test]
fn shipped_tier_builds_type_only_imports_and_explains_value_imports() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let run = |ext: &Path| {
        Command::new(Q2_BIN)
            .args(["call", "build-ts-extension"])
            .arg(ext)
            .env("ESBUILD_BINARY_PATH", workspace_esbuild())
            .output()
            .expect("spawn q2")
    };

    let typed = tmp.path().join("typed");
    write_extension(
        &typed,
        "import type { ExecutionEngineDiscovery } from \"@quarto/types\";\nconst e = { name: \"t\" } as unknown as ExecutionEngineDiscovery;\nexport default e;\n",
    );
    let out = run(&typed);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(typed.join("dist/ext.js").exists());

    let valued = tmp.path().join("valued");
    write_extension(
        &valued,
        "import { primary } from \"@quarto/api/claims\";\nexport default primary;\n",
    );
    let out = run(&valued);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--workspace"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
