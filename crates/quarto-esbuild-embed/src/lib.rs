//! The esbuild binary for the host target, embedded in the `q2` executable.
//!
//! `q2 call build-ts-extension` spawns esbuild directly (no Node, no Deno).
//! Release builds embed the binary (`cargo xtask fetch-esbuild` stages it);
//! [`extract`] writes it once to a per-user cache and returns that path.
//! A build without the staged binary embeds an empty placeholder and
//! [`extract`] returns `Ok(None)`.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

static ESBUILD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/esbuild.bin"));

/// Whether a real esbuild binary is embedded (false for a placeholder build).
pub fn is_embedded() -> bool {
    !ESBUILD.is_empty()
}

/// Extract the embedded binary into the per-user cache and return its path,
/// or `Ok(None)` when this build embeds only a placeholder.
pub fn extract() -> Result<Option<PathBuf>> {
    if !is_embedded() {
        return Ok(None);
    }
    let base = dirs::cache_dir()
        .context("cannot determine the user cache directory to extract esbuild into")?;
    extract_into(&base.join("quarto").join("esbuild"), ESBUILD).map(Some)
}

/// Write `bytes` to `<cache_root>/<content hash>/esbuild[.exe]` unless an
/// identical copy is already there, and return the path. The directory name
/// is the content hash, so a new embedded version never reuses a stale copy.
pub fn extract_into(cache_root: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let digest = Sha256::digest(bytes);
    let key: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let dir = cache_root.join(key);
    let name = if cfg!(windows) {
        "esbuild.exe"
    } else {
        "esbuild"
    };
    let path = dir.join(name);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64) {
        return Ok(path);
    }

    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    // Write beside the destination and rename, so a concurrent extraction
    // never exposes a half-written executable.
    let staging = dir.join(format!("{name}.{}.tmp", std::process::id()));
    std::fs::write(&staging, bytes)
        .with_context(|| format!("failed to write {}", staging.display()))?;
    make_executable(&staging)?;
    std::fs::rename(&staging, &path)
        .with_context(|| format!("failed to move esbuild into place at {}", path.display()))?;
    ensure_signed(&path);
    Ok(path)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("failed to chmod {}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

/// macOS refuses to run an unsigned arm64 binary. The npm package ships a
/// signed binary and extraction preserves its bytes, so this normally passes;
/// if the signature does not verify, ad-hoc sign the extracted copy.
#[cfg(target_os = "macos")]
fn ensure_signed(path: &Path) {
    use std::process::Command;
    let verified = Command::new("codesign")
        .arg("-v")
        .arg(path)
        .output()
        .is_ok_and(|o| o.status.success());
    if !verified {
        let _ = Command::new("codesign")
            .args(["--force", "-s", "-"])
            .arg(path)
            .output();
    }
}

#[cfg(not(target_os = "macos"))]
fn ensure_signed(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_is_content_addressed_and_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let a = extract_into(root.path(), b"binary-one").unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"binary-one");
        assert_eq!(extract_into(root.path(), b"binary-one").unwrap(), a);

        let b = extract_into(root.path(), b"binary-two!").unwrap();
        assert_ne!(a, b, "different bytes must not reuse a cached copy");
        assert_eq!(std::fs::read(&a).unwrap(), b"binary-one");
    }

    #[cfg(unix)]
    #[test]
    fn extracted_binary_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = extract_into(root.path(), b"x").unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }

    #[test]
    fn placeholder_build_embeds_nothing() {
        // The test build runs without a staged binary unless the developer
        // fetched one; both outcomes must be self-consistent.
        assert_eq!(extract().unwrap().is_some(), is_embedded());
    }
}
