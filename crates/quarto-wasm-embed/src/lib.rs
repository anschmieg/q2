//! The single embedded copy of the `wasm-quarto-hub-client` render
//! WASM (`wasm_quarto_hub_client_bg.wasm`) for the q2 binary.
//!
//! Two consumers share this crate so the ~36 MB WASM rides the binary
//! exactly once (plan
//! `claude-notes/plans/2026-10-13-wasm-embed-dedupe.md`):
//!
//! - `q2 preview` (`quarto-preview`): its build.rs strips the
//!   Vite-hashed WASM from the viewer embed archive and
//!   `lookup_embedded` serves these bytes at that recorded path.
//! - `q2 mcp` (`quarto-mcp-launcher`): its build.rs strips the WASM
//!   from the embedded MCP bundle and the launcher appends these bytes
//!   to the extraction set, materializing [`WASM_FILENAME`] next to
//!   `wasm-host.mjs` in the per-user cache dir.
//!
//! `build.rs` embeds the selected source file zstd-compressed; the
//! identity bytes are produced lazily on first call, so `q2 render`-
//! class invocations that never serve or extract the WASM pay nothing
//! (no CPU, no RSS) — the same laziness contract as the preview's
//! tar.zst embeds (bd-rem4bpee).
//!
//! On a tree with neither candidate (fresh clone, no built dists) the
//! embedded archive is empty and [`wasm_bytes`] returns `None`; the
//! consumers' placeholder paths carry the actionable messages.

use std::sync::OnceLock;

/// The fixed filename every consumer materializes the WASM under:
/// `wasm-host.mjs` reads `new URL('./wasm_quarto_hub_client_bg.wasm',
/// import.meta.url)`, so the launcher extracts it beside the host.
pub const WASM_FILENAME: &str = "wasm_quarto_hub_client_bg.wasm";

/// The build-time embedded archive (zstd frame; empty on a placeholder
/// build). See `build.rs` for the source-file scan order.
static WASM_ARCHIVE: &[u8] = include_bytes!(env!("QUARTO_WASM_EMBED_ARCHIVE"));

/// Lowercase-hex SHA-256 of the embedded WASM, stamped at build time
/// from the very bytes [`WASM_ARCHIVE`] compresses; `None` on a
/// placeholder build. Lets consumers key caches and staleness checks on
/// the WASM's identity without decoding or hashing it (the `q2 mcp`
/// launcher's warm-cache path must not pay the ~36 MB decode/hash).
pub const WASM_SHA256: Option<&str> = option_env!("QUARTO_WASM_EMBED_SHA256");

/// Identity (uncompressed) byte length of the embedded WASM, stamped
/// at build time alongside [`WASM_SHA256`]; `None` on a placeholder
/// build.
pub const WASM_LEN: Option<u64> = match option_env!("QUARTO_WASM_EMBED_LEN") {
    Some(s) => Some(parse_u64(s)),
    None => None,
};

/// The build script emits a decimal byte count; parse it in const
/// context (`u64::from_str` is not const).
const fn parse_u64(s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut n = 0u64;
    let mut i = 0;
    while i < bytes.len() {
        n = n * 10 + (bytes[i] - b'0') as u64;
        i += 1;
    }
    n
}

/// Lazily decompressed identity bytes. `None` on a placeholder build.
static WASM_BYTES: OnceLock<Option<Box<[u8]>>> = OnceLock::new();

/// The embedded render WASM as identity (uncompressed) bytes, or `None`
/// when this binary was built without a WASM source (fresh clone).
/// Decompression happens once per process, on first call.
pub fn wasm_bytes() -> Option<&'static [u8]> {
    WASM_BYTES
        .get_or_init(|| {
            if WASM_ARCHIVE.is_empty() {
                return None;
            }
            // The archive is written by this crate's own build.rs, so a
            // decode failure is a build bug, not a runtime condition.
            let bytes =
                zstd::stream::decode_all(WASM_ARCHIVE).expect("embedded wasm.zst must decode");
            Some(bytes.into_boxed_slice())
        })
        .as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both-tree-states smoke test: on a built tree the embed must
    /// decode to real WASM bytes; on a fresh clone it must be None.
    /// Either way the call must be idempotent.
    #[test]
    fn wasm_bytes_matches_tree_state() {
        match wasm_bytes() {
            Some(bytes) => {
                assert!(
                    bytes.starts_with(b"\0asm"),
                    "embedded WASM must carry the wasm magic header"
                );
                assert!(
                    bytes.len() > 1_000_000,
                    "the render WASM is tens of MB; {} bytes is implausible",
                    bytes.len()
                );
                assert!(
                    std::ptr::eq(bytes, wasm_bytes().expect("second call")),
                    "repeated calls must return the same cached bytes"
                );
            }
            None => {
                assert!(wasm_bytes().is_none(), "placeholder stays placeholder");
            }
        }
    }

    /// The build-time stamped identity (sha256, length) must agree with
    /// the decoded payload — the launcher keys its extraction cache and
    /// its staleness warning on the stamp precisely so it never has to
    /// decode or hash the ~36 MB on a warm cache.
    #[test]
    fn stamped_identity_matches_decoded_bytes() {
        match (wasm_bytes(), WASM_SHA256, WASM_LEN) {
            (Some(bytes), Some(sha), Some(len)) => {
                assert_eq!(bytes.len() as u64, len, "stamped length");
                assert_eq!(sha256_hex(bytes), sha, "stamped sha256");
                assert_eq!(sha.len(), 64, "lowercase hex sha256");
            }
            // Placeholder build: stamp and payload are absent together.
            (None, None, None) => {}
            (bytes, sha, len) => panic!(
                "stamp and payload must agree: bytes={:?} sha={:?} len={:?}",
                bytes.map(|b| b.len()),
                sha,
                len
            ),
        }
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn wasm_filename_matches_the_host_glue_expectation() {
        // wasm-host-entry.mjs reads `new URL('./<name>', import.meta.url)`;
        // pin the name the launcher extracts so the two cannot drift.
        assert_eq!(WASM_FILENAME, "wasm_quarto_hub_client_bg.wasm");
        let entry = include_str!("../../../ts-packages/quarto-hub-mcp/scripts/wasm-host-entry.mjs");
        assert!(
            entry.contains(WASM_FILENAME),
            "wasm-host-entry.mjs must read the same sibling filename"
        );
    }
}
