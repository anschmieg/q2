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
