//! Dynamic dependency enrollment for single-file preview.
//!
//! Single-file sessions intentionally never traverse/sync sibling directories
//! or broaden the native watcher beyond its boot-time closure. When the author
//! adds a new include to the deck or creates a missing include, however,
//! those new files would otherwise never reach the WASM VFS. A bounded native
//! rescan uses the *existing renderer include-expansion stages* to discover
//! only actual dependencies, enrolls them in the live Automerge index, and
//! syncs later edits to enrolled files. No second shortcode parser.
//!
//! This task is restricted to single-file mode, and exits on server shutdown.
//! The 2s interval is a correctness fallback; the native watcher still delivers
//! ordinary known-dependency edits promptly. Future event-driven refresh can
//! replace polling without changing the trusted enrollment boundary.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use quarto_hub::HubContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tokio::sync::watch;

pub(crate) async fn run(
    ctx: Arc<HubContext>,
    root: PathBuf,
    deck: PathBuf,
    initial_dependencies: Vec<PathBuf>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut tracked: BTreeSet<PathBuf> = initial_dependencies.into_iter().collect();
    let mut ticker = tokio::time::interval(Duration::from_secs(2));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Initial closure is already discovered and synced at server boot.
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let root_for_scan = root.clone();
                let deck_for_scan = deck.clone();
                let discovered = tokio::task::spawn_blocking(move || {
                    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
                    crate::config::resolve_single_file_deps(
                        &root_for_scan, &deck_for_scan, runtime,
                    )
                }).await;
                let Ok(deps) = discovered else {
                    tracing::warn!("single-file dependency refresh worker failed");
                    continue;
                };

                if let Err(error) = ctx
                    .enroll_preview_dependencies(&deps.qmd_files, &deps.binary_files)
                    .await
                {
                    tracing::warn!(%error, "single-file dependency enrollment failed");
                    continue;
                }
                let current: BTreeSet<PathBuf> = deps.qmd_files.iter()
                    .chain(deps.binary_files.iter()).cloned().collect();
                // Do not prune on partial scans: malformed edits and broken
                // includes must not silently discard still-existing inputs.
                // A genuinely disappeared file can be retired even when
                // include expansion fails, allowing a missing-include render
                // error to reach the browser via index membership changes.
                let retired: Vec<PathBuf> = tracked.difference(&current)
                    .filter(|path| deps.complete || matches!(
                        std::fs::canonicalize(root.join(path)),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound
                    ))
                    .cloned().collect();
                // Existing dependencies still use the low-latency native watcher.
                // This sync fallback covers *newly enrolled* files as well as
                // newly edited transitive dependencies that weren't in its
                // immutable boot-time allowlist. The hash checkpoint in
                // HubContext::sync_file makes unchanged files a cheap no-op.
                for path in deps.qmd_files.iter().chain(deps.binary_files.iter()) {
                    if let Err(error) = ctx.sync_file(&root.join(path)).await {
                        tracing::debug!(
                            path = %path.display(),
                            %error,
                            "single-file dependency refresh will retry",
                        );
                    }
                }
                if !retired.is_empty() {
                    if let Err(error) = ctx.retire_preview_dependencies(&retired, &deck).await {
                        tracing::warn!(%error, "single-file dependency retirement failed");
                        continue;
                    }
                }
                tracked.extend(current);
                for path in retired { tracked.remove(&path); }
            }
            event = shutdown.changed() => {
                if event.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}
