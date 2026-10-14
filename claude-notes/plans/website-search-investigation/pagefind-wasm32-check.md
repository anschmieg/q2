---
title: 'Pagefind 1.5.2 wasm32 fork feasibility (check-only experiment)'
date: 2026-10-14
description: 'Measured result of patching a scratch copy of the pagefind 1.5.2 crate until cargo check passes for wasm32-unknown-unknown. Supports decision 6 of the website search plan (bd-eip0naf4): cfg-gate now, do not fork, but know that a fork would work.'
braid:
  strand: bd-eip0naf4
---

Supporting artifact for `claude-notes/plans/2026-10-14-website-search.md`, § wasm32 fork feasibility. Nothing here is committed to the q2 tree as code; the experiment lived in a scratch copy of the crate.

## Result

`cargo check --lib --no-default-features --target wasm32-unknown-unknown` passes after an 82-line patch (6 files, +29 / −53). Native checks with and without the default `serve` feature still pass, so the patch does not break the CLI build.

## Baseline errors, in the order they surface

1. `tokio` build script: "Only features sync,macros,io-util,rt,time are supported on wasm" (the crate asks for `rt-multi-thread` and `fs`). Hard error; nothing downstream compiles.
2. With tokio fixed, exactly one error in pagefind itself: `tokio::fs` imported in `src/output/mod.rs`.

There was no third layer. `rayon`, `wax`, `console`, `twelf`, `clap`, `lol_html`, `flate2`, `include_dir`, `minicbor`, `pagefind_stem`, `regex`, `sha-1` all type-check for wasm32 unchanged. `async-compression` is declared but never used in `src/`. The known 1.5.2 bug (default features off needs `tokio/io-util`) reproduced natively and is fixed by adding that feature.

## What the patch changes

- `Cargo.toml`: tokio features reduced to `rt, time, macros, sync, io-util`; `rayon` and `async-compression` removed. A further probe showed `tokio = { features = ["sync"] }` alone passes both targets; `rt`/`macros` are only needed by the `#[tokio::test]` tests.
- `src/output/mod.rs`: `tokio::fs` + `tokio::time::sleep` replaced by `std::fs` + `std::thread::sleep` on the write-to-disk path only; the in-memory path used by `get_files()` is untouched.
- `src/lib.rs`: the rayon `into_par_iter().partition_map(...)` in `fossick_many` becomes a sequential loop; `runner` and `serve` gated `cfg(not(target_arch = "wasm32"))`.
- `src/index/mod.rs`: three `into_par_iter()` become `into_iter()`.
- `src/service/api.rs`: three `spawn_blocking` wrappers become direct `lock().await` calls; `tokio::sync::Mutex` kept.
- `src/service/mod.rs`: `run_service` (stdin/stdout threads, `process::exit`) gated out.

Untouched: `src/fossick/*`, `src/fragments/`, `src/output/entry.rs`, `src/index/index_*.rs`, `src/options.rs`.

## Caveats a real fork would still have to handle

- A passing check is weaker than a working port. Code that compiles for wasm32 but panics at runtime (thread spawn, blocking sleep, `Instant::now()`) was sequentialised or gated here rather than stubbed; one `std::thread::sleep` remains in the disk-read retry loop, which the in-memory path never reaches.
- `include_dir!` of `vendor/wasm` embeds 33 language search runtimes (about 2.5 MB; `vendor/` is 3.1 MB in total) into whatever binary embeds the crate. A hub-client indexer would want a single-language `include_bytes!` or an out-of-band fetch (about 20 lines in `write_common_*`).
- `clap`, `twelf`, `console` and `wax` stay in the wasm tree because `options.rs` derives the inbound config from them: bloat, not a blocker.
- 17 dead-code warnings on wasm32 from gating `run_service`; a real fork gates `service/requests.rs` and `responses.rs` too.

## Assessment

Small mechanical patch, not a refactor: budget about a day including tests for the real fork, plus trimming work for bundle size. The toolchain used was the same nightly q2 pins (it has the wasm32 target installed); no components were installed.

## The patch

```diff
diff --git a/Cargo.toml b/Cargo.toml
index 6c52a35..d4169b8 100644
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -49,13 +49,6 @@ optional = true
 [dependencies.anyhow]
 version = "1.0"
 
-[dependencies.async-compression]
-version = "0.4"
-features = [
-    "tokio",
-    "gzip",
-]
-
 [dependencies.base64]
 version = "0.22"
 
@@ -166,9 +159,6 @@ version = "0.2"
 version = "0.1"
 optional = true
 
-[dependencies.rayon]
-version = "1.10"
-
 [dependencies.regex]
 version = "1.1"
 
@@ -188,8 +178,7 @@ version = "0.10"
 [dependencies.tokio]
 version = "1"
 features = [
-    "rt-multi-thread",
-    "fs",
+    "rt",
     "time",
     "macros",
     "sync",
diff --git a/src/index/mod.rs b/src/index/mod.rs
index a62f67c..ad3a4d0 100644
--- a/src/index/mod.rs
+++ b/src/index/mod.rs
@@ -1,5 +1,4 @@
 use hashbrown::HashMap;
-use rayon::prelude::*;
 use std::collections::{BTreeMap, BTreeSet};
 
 use crate::{
@@ -147,7 +146,7 @@ pub async fn build_indexes(
 
     // Process pages in parallel - each page builds its own word map
     let page_results: Vec<PageProcessingResult> = pages
-        .into_par_iter()
+        .into_iter()
         .map(|page| {
             let mut local_word_map: HashMap<String, PackedWord> = HashMap::new();
 
@@ -407,7 +406,7 @@ pub async fn build_indexes(
     // Convert hashbrown HashMap to Vec for rayon compatibility
     let filter_map_vec: Vec<_> = filter_map.into_iter().collect();
     let encoded_filters: Vec<(String, Vec<u8>, String)> = filter_map_vec
-        .into_par_iter()
+        .into_iter()
         .map(|(filter, values)| {
             let mut filter_index: Vec<u8> = Vec::new();
             let _ = minicbor::encode::<FilterIndex, &mut Vec<u8>>(
@@ -466,7 +465,7 @@ pub async fn build_indexes(
 
     // Encode word index chunks in parallel (delta encoding + CBOR serialization)
     let encoded_chunks: Vec<(usize, Vec<u8>, String)> = chunks
-        .into_par_iter()
+        .into_iter()
         .enumerate()
         .map(|(i, chunk)| {
             // Delta-encode page numbers within each word's page list
diff --git a/src/lib.rs b/src/lib.rs
index 668dbdf..511c8f5 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,14 +1,12 @@
 use std::{cmp::Ordering, path::PathBuf};
 
 use anyhow::{bail, Result};
-use either::Either;
 use fossick::{FossickedData, Fossicker};
 use futures::future::join_all;
 use hashbrown::HashMap;
 use index::PagefindIndexes;
 use options::{PagefindInboundConfig, SearchOptions};
 use output::SyntheticFile;
-use rayon::prelude::*;
 pub use service::api;
 use wax::walk::Entry;
 use wax::Glob;
@@ -23,8 +21,9 @@ mod logging;
 pub mod options;
 mod output;
 mod playground;
+#[cfg(not(target_arch = "wasm32"))]
 pub mod runner;
-#[cfg(feature = "serve")]
+#[cfg(all(feature = "serve", not(target_arch = "wasm32")))]
 mod serve;
 mod service;
 mod utils;
@@ -84,13 +83,14 @@ impl SearchState {
 
         // Use rayon for parallel HTML parsing - this is the main performance win
         // Partition results into successes and failures to report errors
-        let (results, errors): (Vec<_>, Vec<_>) = files
-            .into_par_iter()
-            .map(|f| f.fossick_sync(&self.options))
-            .partition_map(|r| match r {
-                Ok(data) => Either::Left(data),
-                Err(e) => Either::Right(e),
-            });
+        let mut results = Vec::new();
+        let mut errors = Vec::new();
+        for r in files.into_iter().map(|f| f.fossick_sync(&self.options)) {
+            match r {
+                Ok(data) => results.push(data),
+                Err(e) => errors.push(e),
+            }
+        }
 
         // Report any errors that occurred during parallel processing
         if !errors.is_empty() {
diff --git a/src/output/mod.rs b/src/output/mod.rs
index 4f83635..a14feae 100644
--- a/src/output/mod.rs
+++ b/src/output/mod.rs
@@ -10,9 +10,7 @@ use futures::future::join_all;
 use hashbrown::HashMap;
 use include_dir::{include_dir, Dir};
 use minifier::js::minify;
-use tokio::fs::{create_dir_all, File};
-use tokio::io::AsyncWriteExt;
-use tokio::time::sleep;
+use std::fs::{create_dir_all, File};
 
 mod entry;
 
@@ -419,13 +417,13 @@ async fn write(
         WriteBehavior::Immutable if filename.exists() => return None,
         WriteBehavior::Immutable | WriteBehavior::Disk => {
             if let Some(parent) = filename.parent() {
-                create_dir_all(parent).await.unwrap();
+                create_dir_all(parent).unwrap();
             }
 
-            let mut output_file = File::create(&filename).await;
+            let mut output_file = File::create(&filename);
             while output_file.is_err() {
-                sleep(Duration::from_millis(100)).await;
-                output_file = File::create(&filename).await;
+                std::thread::sleep(Duration::from_millis(100));
+                output_file = File::create(&filename);
             }
             file = output_file.ok();
         }
@@ -440,7 +438,7 @@ async fn write(
             }
             if let Ok(contents) = gz.finish() {
                 if let Some(mut file) = file {
-                    file.write_all(&contents).await.unwrap();
+                    file.write_all(&contents).unwrap();
                 } else {
                     return Some(SyntheticFile { filename, contents });
                 }
@@ -450,7 +448,7 @@ async fn write(
         Compress::None => {
             if let Some(mut file) = file {
                 for chunk in content_chunks {
-                    file.write_all(chunk).await.unwrap();
+                    file.write_all(chunk).unwrap();
                 }
                 None
             } else {
diff --git a/src/service/api.rs b/src/service/api.rs
index 724dc94..9c29f15 100644
--- a/src/service/api.rs
+++ b/src/service/api.rs
@@ -99,12 +99,7 @@ impl PagefindIndex {
         }
 
         let file = Fossicker::new_synthetic(source_path.map(PathBuf::from), url, content);
-        let state = Arc::clone(&self.search_index);
-        let data = tokio::task::spawn_blocking(move || {
-            state.blocking_lock().fossick_one(file)
-        })
-        .await
-        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))??;
+        let data = self.search_index.lock().await.fossick_one(file)?;
 
         Ok(IndexedFileResponse {
             page_word_count: data.fragment.data.word_count as u32,
@@ -155,12 +150,7 @@ impl PagefindIndex {
             language: force_language.unwrap_or(language),
         };
         let file = Fossicker::new_with_data(url, data);
-        let state = Arc::clone(&self.search_index);
-        let data = tokio::task::spawn_blocking(move || {
-            state.blocking_lock().fossick_one(file)
-        })
-        .await
-        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))??;
+        let data = self.search_index.lock().await.fossick_one(file)?;
 
         Ok(IndexedFileResponse {
             page_word_count: data.fragment.data.word_count as u32,
@@ -182,12 +172,11 @@ impl PagefindIndex {
             serde_json::from_str("{}").expect("All fields have serde defaults");
         let glob = glob.unwrap_or(defaults.glob);
 
-        let state = Arc::clone(&self.search_index);
-        let page_count = tokio::task::spawn_blocking(move || {
-            state.blocking_lock().fossick_many(PathBuf::from(path), glob)
-        })
-        .await
-        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))??;
+        let page_count = self
+            .search_index
+            .lock()
+            .await
+            .fossick_many(PathBuf::from(path), glob)?;
 
         Ok(page_count)
     }
diff --git a/src/service/mod.rs b/src/service/mod.rs
index 41d772a..79fe633 100644
--- a/src/service/mod.rs
+++ b/src/service/mod.rs
@@ -12,6 +12,7 @@ use responses::*;
 mod requests;
 mod responses;
 
+#[cfg(not(target_arch = "wasm32"))]
 pub async fn run_service() {
     let (incoming_tx, mut incoming_rx) = mpsc::unbounded_channel::<ServiceRequest>();
     let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<ServiceResponse>();
```
