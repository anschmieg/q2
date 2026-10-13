//! `cargo xtask fetch-esbuild --target <triple>` — stage the esbuild binary
//! that `quarto-esbuild-embed` embeds in the `q2` executable.
//!
//! Downloads `@esbuild/<platform>@<version>` with `npm pack`, checks the
//! tarball against the integrity hash pinned in the root `package-lock.json`,
//! and writes `ts-packages/esbuild-embed/<triple>/esbuild[.exe]` (gitignored).
//! The version therefore always equals the esbuild the TS packages build with.
//!
//! Called by `cargo xtask build-all` for the host target and by the release
//! pipeline's per-target leg (which cross-builds, so `--target` is explicit).

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use sha2::{Digest, Sha512};
use std::path::{Path, PathBuf};

use crate::util::nested_command;

pub struct Args {
    /// Rust target triple; defaults to the host.
    pub target: Option<String>,
}

/// The `@esbuild/<name>` package for a Rust target triple, and the binary's
/// path inside the package tarball.
pub fn platform_for_target(triple: &str) -> Result<(&'static str, &'static str)> {
    let name = match triple {
        "aarch64-apple-darwin" => "darwin-arm64",
        "x86_64-apple-darwin" => "darwin-x64",
        "x86_64-unknown-linux-gnu" | "x86_64-unknown-linux-musl" => "linux-x64",
        "aarch64-unknown-linux-gnu" | "aarch64-unknown-linux-musl" => "linux-arm64",
        "x86_64-pc-windows-msvc" => "win32-x64",
        "aarch64-pc-windows-msvc" => "win32-arm64",
        other => bail!("no esbuild platform package is known for target {other}"),
    };
    let bin = if triple.contains("windows") {
        "package/esbuild.exe"
    } else {
        "package/bin/esbuild"
    };
    Ok((name, bin))
}

/// `(version, integrity)` of `node_modules/@esbuild/<platform>` in a lockfile.
pub fn lock_entry(lock: &serde_json::Value, platform: &str) -> Result<(String, String)> {
    let entry = &lock["packages"][format!("node_modules/@esbuild/{platform}")];
    let field = |name: &str| {
        entry[name]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("package-lock.json has no {name} for @esbuild/{platform}"))
    };
    Ok((field("version")?, field("integrity")?))
}

/// Check `bytes` against an npm `sha512-<base64>` integrity string.
pub fn verify_integrity(bytes: &[u8], integrity: &str) -> Result<()> {
    let expected = integrity
        .strip_prefix("sha512-")
        .ok_or_else(|| anyhow!("unsupported integrity format: {integrity}"))?;
    let actual = base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes));
    if actual != expected {
        bail!("integrity mismatch: expected sha512-{expected}, got sha512-{actual}");
    }
    Ok(())
}

/// The host's Rust target triple (what `cargo build` targets by default).
fn host_triple() -> Result<String> {
    let out = std::process::Command::new("rustc")
        .arg("-vV")
        .output()
        .context("failed to run `rustc -vV`")?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
        .ok_or_else(|| anyhow!("`rustc -vV` printed no host triple"))
}

pub fn run(args: Args) -> Result<()> {
    let root = project_root()?;
    let triple = match args.target {
        Some(t) => t,
        None => host_triple()?,
    };
    let (platform, bin_in_tarball) = platform_for_target(&triple)?;

    let lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("package-lock.json"))
            .context("failed to read package-lock.json")?,
    )?;
    let (version, integrity) = lock_entry(&lock, platform)?;

    let dest_dir = root.join("ts-packages/esbuild-embed").join(&triple);
    let exe = if triple.contains("windows") {
        "esbuild.exe"
    } else {
        "esbuild"
    };
    let dest = dest_dir.join(exe);

    println!("━━━ Fetching @esbuild/{platform}@{version} for {triple} ━━━");
    let scratch = tempfile::tempdir()?;
    let pack = nested_command("npm")
        .args([
            "pack",
            &format!("@esbuild/{platform}@{version}"),
            "--silent",
            "--pack-destination",
        ])
        .arg(scratch.path())
        .output()
        .context("failed to run `npm pack`")?;
    if !pack.status.success() {
        bail!("npm pack failed: {}", String::from_utf8_lossy(&pack.stderr));
    }
    let tarball = find_tarball(scratch.path())?;
    verify_integrity(&std::fs::read(&tarball)?, &integrity)
        .with_context(|| format!("{} does not match package-lock.json", tarball.display()))?;

    let status = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(scratch.path())
        .arg(bin_in_tarball)
        .status()
        .context("failed to run `tar`")?;
    if !status.success() {
        bail!(
            "tar could not extract {bin_in_tarball} from {}",
            tarball.display()
        );
    }

    std::fs::create_dir_all(&dest_dir)?;
    std::fs::copy(scratch.path().join(bin_in_tarball), &dest)
        .with_context(|| format!("failed to write {}", dest.display()))?;
    // npm tarball entries carry a 1985 mtime, which cargo reads as "older than
    // the last build" and so never reruns the embed crate's build script.
    std::fs::File::options()
        .write(true)
        .open(&dest)?
        .set_modified(std::time::SystemTime::now())?;
    println!("✓ {}", dest.display());
    Ok(())
}

fn find_tarball(dir: &Path) -> Result<PathBuf> {
    std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "tgz"))
        .ok_or_else(|| anyhow!("npm pack produced no .tgz in {}", dir.display()))
}

fn project_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("failed to get current directory")?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() && std::fs::read_to_string(&cargo_toml)?.contains("[workspace]") {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not find workspace root (Cargo.toml with [workspace])");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_release_target_to_a_platform_package() {
        for (triple, name) in [
            ("aarch64-apple-darwin", "darwin-arm64"),
            ("x86_64-apple-darwin", "darwin-x64"),
            ("x86_64-unknown-linux-gnu", "linux-x64"),
            ("x86_64-unknown-linux-musl", "linux-x64"),
            ("aarch64-unknown-linux-gnu", "linux-arm64"),
            ("x86_64-pc-windows-msvc", "win32-x64"),
        ] {
            assert_eq!(platform_for_target(triple).unwrap().0, name, "{triple}");
        }
        assert!(platform_for_target("riscv64gc-unknown-linux-gnu").is_err());
    }

    #[test]
    fn windows_binary_lives_at_the_package_root() {
        assert_eq!(
            platform_for_target("x86_64-pc-windows-msvc").unwrap().1,
            "package/esbuild.exe"
        );
        assert_eq!(
            platform_for_target("aarch64-apple-darwin").unwrap().1,
            "package/bin/esbuild"
        );
    }

    #[test]
    fn reads_version_and_integrity_from_the_lockfile() {
        let lock = serde_json::json!({"packages": {"node_modules/@esbuild/linux-x64": {
            "version": "0.28.2", "integrity": "sha512-abc"
        }}});
        assert_eq!(
            lock_entry(&lock, "linux-x64").unwrap(),
            ("0.28.2".to_string(), "sha512-abc".to_string())
        );
        assert!(lock_entry(&lock, "darwin-arm64").is_err());
    }

    #[test]
    fn the_repo_lockfile_pins_every_platform_package() {
        let lock: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../package-lock.json"),
            )
            .unwrap(),
        )
        .unwrap();
        for triple in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ] {
            let (platform, _) = platform_for_target(triple).unwrap();
            let (version, integrity) = lock_entry(&lock, platform).unwrap();
            assert!(integrity.starts_with("sha512-"), "{triple}: {integrity}");
            assert_eq!(version, "0.28.2", "{triple}");
        }
    }

    #[test]
    fn integrity_check_accepts_a_match_and_rejects_a_mismatch() {
        let digest = base64::engine::general_purpose::STANDARD.encode(Sha512::digest(b"payload"));
        verify_integrity(b"payload", &format!("sha512-{digest}")).unwrap();
        assert!(verify_integrity(b"tampered", &format!("sha512-{digest}")).is_err());
        assert!(verify_integrity(b"payload", "sha1-xyz").is_err());
    }
}
