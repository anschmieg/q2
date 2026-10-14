//! The embedded hub MCP server bundle.
//!
//! `build.rs` points `QUARTO_HUB_MCP_EMBED_DIR` either at the real
//! `ts-packages/quarto-hub-mcp/dist-bundle/` or at a placeholder
//! containing only a `BUNDLE_NOT_BUILT` marker (fresh clone, no npm
//! step yet).

use include_dir::{Dir, include_dir};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::PathBuf;

static EMBEDDED_BUNDLE: Dir<'_> = include_dir!("$QUARTO_HUB_MCP_EMBED_DIR");

/// One file of the bundle: workspace-relative path (forward slashes,
/// as `include_dir` stores them) plus contents. The extraction and
/// hashing code works on this representation so tests can fabricate
/// bundles without compile-time embedding.
pub type BundleFile = (PathBuf, BundleContents);

/// The contents of one bundle file. Most files embed their bytes
/// directly; the shared render WASM rides as [`BundleContents::Lazy`]
/// so a `q2 mcp` launch never decodes or hashes its ~36 MB unless an
/// extraction actually writes it — warm-cache launches are the common
/// case, and [`content_hash`] keys the entry by its build-time pinned
/// digest instead (plan
/// `claude-notes/plans/2026-10-13-wasm-embed-dedupe.md`, follow-up).
#[derive(Clone, Debug)]
pub enum BundleContents {
    /// Already-materialized bytes (borrowed from the embed, or owned).
    Bytes(Cow<'static, [u8]>),
    /// A build-time-identified payload materialized on first use.
    Lazy(LazyBytes),
}

/// A lazily-materialized bundle payload whose identity is pinned by a
/// build-time stamped digest (the shared render WASM from
/// `quarto-wasm-embed`).
#[derive(Clone, Copy, Debug)]
pub struct LazyBytes {
    /// Lowercase-hex SHA-256 of the payload (build-time stamped).
    pub sha256: &'static str,
    /// Identity (uncompressed) length in bytes.
    pub len: u64,
    /// Materializes the payload. `None` means this binary embeds no
    /// payload — the launcher validates presence before constructing a
    /// Lazy entry, so reaching it is a build bug.
    pub get: fn() -> Option<&'static [u8]>,
}

impl BundleContents {
    /// The file's bytes, materializing a Lazy payload on first call.
    /// `None` only from a Lazy provider with no payload (see above).
    pub fn materialize(&self) -> Option<&[u8]> {
        match self {
            BundleContents::Bytes(bytes) => Some(bytes.as_ref()),
            BundleContents::Lazy(lazy) => (lazy.get)(),
        }
    }
}

/// True when the build embedded the placeholder instead of a real
/// bundle (`cargo build` before `cargo xtask build-hub-mcp-bundle`).
pub fn is_placeholder() -> bool {
    EMBEDDED_BUNDLE.get_file("BUNDLE_NOT_BUILT").is_some()
}

/// Flatten the embedded directory tree into (path, contents) pairs.
pub fn embedded_files() -> Vec<BundleFile> {
    let mut files = Vec::new();
    collect(&EMBEDDED_BUNDLE, &mut files);
    files
}

fn collect(dir: &Dir<'static>, out: &mut Vec<BundleFile>) {
    for file in dir.files() {
        out.push((
            file.path().to_path_buf(),
            BundleContents::Bytes(Cow::Borrowed(file.contents())),
        ));
    }
    for sub in dir.dirs() {
        collect(sub, out);
    }
}

/// Content hash of a bundle: 16 hex chars of SHA-256 over the sorted
/// (path, length, contents) sequence, where a Lazy entry contributes a
/// marked digest surrogate (`b"sha256\0"` + the pinned hex digest) in
/// place of contents — its identity, without touching its bytes. Used
/// as the cache directory name — same bundle bytes, same directory,
/// across q2 builds.
pub fn content_hash(files: &[BundleFile]) -> String {
    let mut sorted: Vec<&BundleFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, contents) in sorted {
        // Hash the path with forward slashes so the same bundle hashes
        // identically regardless of host separator conventions.
        let path_str = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        hasher.update(path_str.as_bytes());
        hasher.update([0u8]);
        match contents {
            BundleContents::Bytes(bytes) => {
                hasher.update((bytes.len() as u64).to_le_bytes());
                hasher.update(bytes);
            }
            BundleContents::Lazy(lazy) => {
                // The marker keeps the surrogate domain disjoint from
                // real file contents.
                hasher.update(lazy.len.to_le_bytes());
                hasher.update(b"sha256\0");
                hasher.update(lazy.sha256.as_bytes());
            }
        }
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
}

/// The embedded `build-info.json` stamp, raw (pretty JSON written by
/// the bundler), or None for placeholder builds.
pub fn build_info_json() -> Option<&'static str> {
    EMBEDDED_BUNDLE
        .get_file("build-info.json")
        .and_then(|f| f.contents_utf8())
}

/// The `wasmSha256` field of the embedded `build-info.json` — the
/// dedupe marker (plan
/// `claude-notes/plans/2026-10-13-wasm-embed-dedupe.md`): bundles that
/// record it ship WITHOUT the render WASM in the q2 embed (build.rs
/// strips it) and the launcher restores it from `quarto-wasm-embed` at
/// extraction time. `None` for placeholder builds and pre-dedupe
/// bundles (which keep their own WASM in the embed).
pub fn embedded_wasm_sha256() -> Option<String> {
    build_info_json().and_then(parse_wasm_sha256)
}

/// Parse the `wasmSha256` field out of a raw `build-info.json` blob.
fn parse_wasm_sha256(build_info: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(build_info).ok()?;
    Some(value.get("wasmSha256")?.as_str()?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, contents: &[u8]) -> BundleFile {
        (
            PathBuf::from(path),
            BundleContents::Bytes(Cow::Owned(contents.to_vec())),
        )
    }

    /// A Lazy payload whose provider must never be called by the code
    /// under test — materializing it fails the test loudly.
    fn panicking_lazy(sha256: &'static str, len: u64) -> LazyBytes {
        fn explode() -> Option<&'static [u8]> {
            panic!("lazy payload must not be materialized here");
        }
        LazyBytes {
            sha256,
            len,
            get: explode,
        }
    }

    /// The content hash keys a Lazy entry by its pinned digest — never
    /// by its bytes — so a warm-cache launch pays no decode and no hash
    /// of the payload.
    #[test]
    fn hash_keys_lazy_entries_by_digest_without_materializing() {
        let lazy_entry = |sha: &'static str| {
            (
                PathBuf::from("w.wasm"),
                BundleContents::Lazy(panicking_lazy(sha, 10)),
            )
        };
        let a = vec![file("index.mjs", b"mjs"), lazy_entry("aaaa")];
        let b = vec![file("index.mjs", b"mjs"), lazy_entry("bbbb")];
        assert_ne!(
            content_hash(&a),
            content_hash(&b),
            "the digest is the identity"
        );
        // Same digest, different provider: same cache key.
        fn other() -> Option<&'static [u8]> {
            Some(b"x")
        }
        let c = vec![
            file("index.mjs", b"mjs"),
            (
                PathBuf::from("w.wasm"),
                BundleContents::Lazy(LazyBytes {
                    sha256: "aaaa",
                    len: 10,
                    get: other,
                }),
            ),
        ];
        assert_eq!(content_hash(&a), content_hash(&c));
    }

    /// The digest surrogate lives in a domain disjoint from real file
    /// contents: a Lazy entry and a Bytes entry at the same path key
    /// differently (the surrogate is marked).
    #[test]
    fn hash_separates_lazy_and_bytes_domains() {
        let contents: &'static [u8] = b"aaaa"; // shaped like a raw digest
        let as_bytes = vec![(
            PathBuf::from("w.wasm"),
            BundleContents::Bytes(Cow::Borrowed(contents)),
        )];
        let as_lazy = vec![(
            PathBuf::from("w.wasm"),
            BundleContents::Lazy(panicking_lazy("aaaa", contents.len() as u64)),
        )];
        assert_ne!(content_hash(&as_bytes), content_hash(&as_lazy));
    }

    #[test]
    fn hash_is_order_independent() {
        let a = vec![file("a.txt", b"one"), file("b/c.txt", b"two")];
        let b = vec![file("b/c.txt", b"two"), file("a.txt", b"one")];
        assert_eq!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn hash_changes_with_content() {
        let a = vec![file("a.txt", b"one")];
        let b = vec![file("a.txt", b"two")];
        assert_ne!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn hash_distinguishes_path_vs_content_boundary() {
        // ("ab", "c") must not collide with ("a", "bc")
        let a = vec![file("ab", b"c")];
        let b = vec![file("a", b"bc")];
        assert_ne!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn hash_is_16_hex_chars() {
        let h = content_hash(&[file("x", b"y")]);
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn parse_wasm_sha256_reads_the_field() {
        let info = "{\n  \"gitCommit\": \"abc\",\n  \"wasmSha256\": \"deadbeef00\",\n  \"keyringPackages\": []\n}\n";
        assert_eq!(parse_wasm_sha256(info).as_deref(), Some("deadbeef00"));
    }

    #[test]
    fn parse_wasm_sha256_absent_on_pre_dedupe_and_garbage() {
        // Pre-dedupe bundle: no field at all.
        assert_eq!(parse_wasm_sha256("{\"gitCommit\": \"abc\"}"), None);
        // Not JSON, or the field of the wrong shape: no marker.
        assert_eq!(parse_wasm_sha256("not json"), None);
        assert_eq!(parse_wasm_sha256("{\"wasmSha256\": 42}"), None);
    }
}
