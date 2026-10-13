//! Extraction, lifetime-lock, and GC semantics (plan Phase 2,
//! bd-81cfshmw). These encode the safety properties that justify the
//! lock-based design: GC can never delete a bundle in use, crashes
//! release locks automatically, and corrupted/raced cache states heal.

use fs2::FileExt;
use quarto_mcp_launcher::{
    BundleContents, BundleFile, DEFAULT_MAX_AGE, LAST_USED_FILE, LOCK_FILE, LazyBytes,
    extract_and_lock, gc,
};
use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

fn bytes_file(path: &str, contents: &'static [u8]) -> BundleFile {
    (
        PathBuf::from(path),
        BundleContents::Bytes(Cow::Borrowed(contents)),
    )
}

fn test_bundle() -> Vec<BundleFile> {
    vec![
        bytes_file("index.mjs", b"// fake bundle entry\n"),
        bytes_file("build-info.json", b"{}\n"),
        bytes_file(
            "node_modules/@napi-rs/keyring/index.js",
            b"// fake addon loader\n",
        ),
    ]
}

const HASH: &str = "cafebabe01234567";

/// Set a file's mtime into the past (the GC recency signal).
fn age_file(path: &Path, age: Duration) {
    let f = OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() - age).unwrap();
}

fn lock_handle(dir: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join(LOCK_FILE))
        .unwrap()
}

#[test]
fn extraction_creates_payload_and_metadata() {
    let cache = tempfile::tempdir().unwrap();
    let extracted = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();

    assert_eq!(extracted.dir, cache.path().join(HASH));
    assert_eq!(
        fs::read(extracted.dir.join("index.mjs")).unwrap(),
        b"// fake bundle entry\n"
    );
    assert!(
        extracted
            .dir
            .join("node_modules/@napi-rs/keyring/index.js")
            .is_file()
    );
    assert!(extracted.dir.join(LAST_USED_FILE).is_file());
    // no extraction temp dirs left behind
    let leftovers: Vec<_> = fs::read_dir(cache.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn extraction_holds_shared_lock_for_lifetime() {
    let cache = tempfile::tempdir().unwrap();
    let extracted = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();

    // While the ExtractedBundle lives, an exclusive try-lock must fail
    // — this is exactly the probe GC uses before deleting.
    let probe = lock_handle(&extracted.dir);
    assert!(probe.try_lock_exclusive().is_err());

    // Dropping the bundle (process exit, in real life) releases it.
    let dir = extracted.dir.clone();
    drop(extracted);
    let probe2 = lock_handle(&dir);
    assert!(probe2.try_lock_exclusive().is_ok());
}

#[test]
fn second_launch_reuses_existing_dir() {
    let cache = tempfile::tempdir().unwrap();
    let first = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    let marker = first.dir.join("user-marker");
    fs::write(&marker, b"still here").unwrap();
    drop(first);

    let second = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    // Same dir, not re-extracted from scratch.
    assert!(marker.is_file(), "re-extraction clobbered the dir");
    drop(second);
}

#[test]
fn concurrent_extractions_converge() {
    let cache = tempfile::tempdir().unwrap();
    let root = cache.path().to_path_buf();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || extract_and_lock(&root, &test_bundle(), HASH).unwrap())
        })
        .collect();
    for h in handles {
        let extracted = h.join().unwrap();
        assert!(extracted.dir.join("index.mjs").is_file());
    }
    // Exactly the one hash dir; every .tmp-* cleaned up.
    let names: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![HASH.to_string()], "cache dirs: {names:?}");
}

#[test]
fn corrupt_cache_dir_self_heals() {
    let cache = tempfile::tempdir().unwrap();
    // A canonical-path dir with no payload (e.g. interrupted manual
    // tampering): extraction must replace it rather than fail forever.
    fs::create_dir_all(cache.path().join(HASH)).unwrap();
    let extracted = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    assert!(extracted.dir.join("index.mjs").is_file());
}

#[test]
fn gc_removes_old_unlocked_bundles() {
    let cache = tempfile::tempdir().unwrap();
    let old = extract_and_lock(cache.path(), &test_bundle(), "0ldhash000000000").unwrap();
    let old_dir = old.dir.clone();
    drop(old); // lock released
    age_file(&old_dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);

    gc(cache.path(), "current-hash", DEFAULT_MAX_AGE);
    assert!(!old_dir.exists(), "old unlocked bundle should be GC'd");
    // no trash leftovers
    let trash: Vec<_> = fs::read_dir(cache.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".trash-"))
        .collect();
    assert!(trash.is_empty());
}

#[test]
fn gc_spares_recently_used_bundles() {
    let cache = tempfile::tempdir().unwrap();
    let recent = extract_and_lock(cache.path(), &test_bundle(), "recenthash000000").unwrap();
    let dir = recent.dir.clone();
    drop(recent); // unlocked, but young
    gc(cache.path(), "current-hash", DEFAULT_MAX_AGE);
    assert!(dir.exists(), "recently used bundle must survive GC");
}

#[test]
fn gc_spares_locked_bundles_even_when_old() {
    let cache = tempfile::tempdir().unwrap();
    let held = extract_and_lock(cache.path(), &test_bundle(), "l0ckedhash000000").unwrap();
    age_file(&held.dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);

    gc(cache.path(), "current-hash", DEFAULT_MAX_AGE);
    assert!(
        held.dir.join("index.mjs").is_file(),
        "a bundle with a live shared lock must never be GC'd"
    );
}

#[test]
fn gc_never_touches_the_current_hash() {
    let cache = tempfile::tempdir().unwrap();
    let current = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    let dir = current.dir.clone();
    drop(current);
    age_file(&dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);

    gc(cache.path(), HASH, DEFAULT_MAX_AGE);
    assert!(
        dir.exists(),
        "keep_hash dir must survive GC regardless of age"
    );
}

#[test]
fn gc_cleans_stale_tmp_and_trash_leftovers() {
    let cache = tempfile::tempdir().unwrap();
    let stale_tmp = cache.path().join(".tmp-99999-0");
    let stale_trash = cache.path().join(".trash-99999-x");
    fs::create_dir_all(&stale_tmp).unwrap();
    fs::create_dir_all(&stale_trash).unwrap();
    fs::write(stale_tmp.join("partial"), b"x").unwrap();
    // Make them look hours old (dir mtime).
    let two_hours = Duration::from_secs(2 * 60 * 60);
    File::open(&stale_tmp)
        .and_then(|f| f.set_modified(SystemTime::now() - two_hours))
        .ok();
    File::open(&stale_trash)
        .and_then(|f| f.set_modified(SystemTime::now() - two_hours))
        .ok();

    gc(cache.path(), HASH, DEFAULT_MAX_AGE);

    // On platforms where dir-handle mtime setting works (unix), the
    // leftovers must be gone; elsewhere they must at least not break GC.
    #[cfg(unix)]
    {
        assert!(!stale_tmp.exists());
        assert!(!stale_trash.exists());
    }
}

#[test]
fn fresh_tmp_dirs_survive_gc() {
    let cache = tempfile::tempdir().unwrap();
    // A just-created .tmp dir simulates an extraction in flight.
    let live_tmp = cache.path().join(".tmp-12345-0");
    fs::create_dir_all(&live_tmp).unwrap();
    gc(cache.path(), HASH, DEFAULT_MAX_AGE);
    assert!(
        live_tmp.exists(),
        "in-flight extraction dir must not be GC'd"
    );
}

#[test]
fn crashed_instance_releases_lock_making_bundle_collectable() {
    // Simulate "crash" by dropping the lock without any cleanup, then
    // aging the dir: GC must collect it. (The kernel releases flocks on
    // process death; in-process drop models that without forking.)
    let cache = tempfile::tempdir().unwrap();
    let crashed = extract_and_lock(cache.path(), &test_bundle(), "crashedhash00000").unwrap();
    let dir = crashed.dir.clone();
    drop(crashed.lock); // "process died"
    age_file(&dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);

    gc(cache.path(), "current-hash", DEFAULT_MAX_AGE);
    assert!(!dir.exists(), "crash-released bundle must be collectable");
}

#[test]
fn launch_after_gc_re_extracts() {
    let cache = tempfile::tempdir().unwrap();
    let first = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    let dir = first.dir.clone();
    drop(first);
    age_file(&dir.join(LAST_USED_FILE), DEFAULT_MAX_AGE * 2);
    gc(cache.path(), "other-hash", DEFAULT_MAX_AGE);
    assert!(!dir.exists());

    // Next launch simply re-extracts.
    let again = extract_and_lock(cache.path(), &test_bundle(), HASH).unwrap();
    assert!(again.dir.join("index.mjs").is_file());
}

static MATERIALIZATIONS: AtomicU32 = AtomicU32::new(0);
static PAYLOAD: &[u8] = b"lazy wasm stand-in";

fn counting_provider() -> Option<&'static [u8]> {
    MATERIALIZATIONS.fetch_add(1, Ordering::Relaxed);
    Some(PAYLOAD)
}

fn lazy_bundle() -> Vec<BundleFile> {
    let mut bundle = test_bundle();
    bundle.push((
        PathBuf::from("wasm_quarto_hub_client_bg.wasm"),
        BundleContents::Lazy(LazyBytes {
            sha256: "cafe",
            len: PAYLOAD.len() as u64,
            get: counting_provider,
        }),
    ));
    bundle
}

/// The shared render WASM rides the extraction set as a lazy entry:
/// a cold extraction materializes and writes it exactly once, and a
/// warm relaunch — the common case — never touches the payload at all
/// (no decode, no hash of its ~36 MB).
#[test]
fn lazy_payload_materializes_on_cold_extraction_only() {
    let cache = tempfile::tempdir().unwrap();
    let before = MATERIALIZATIONS.load(Ordering::Relaxed);

    let first = extract_and_lock(cache.path(), &lazy_bundle(), HASH).unwrap();
    assert_eq!(
        fs::read(first.dir.join("wasm_quarto_hub_client_bg.wasm")).unwrap(),
        PAYLOAD,
        "cold extraction writes the materialized payload"
    );
    assert_eq!(
        MATERIALIZATIONS.load(Ordering::Relaxed),
        before + 1,
        "cold extraction materializes exactly once"
    );
    drop(first);

    let second = extract_and_lock(cache.path(), &lazy_bundle(), HASH).unwrap();
    assert!(
        second.dir.join("wasm_quarto_hub_client_bg.wasm").is_file(),
        "warm relaunch serves the already-extracted payload"
    );
    assert_eq!(
        MATERIALIZATIONS.load(Ordering::Relaxed),
        before + 1,
        "warm relaunch must not materialize the payload"
    );
    drop(second);
}

/// A Lazy entry whose payload is absent (a build broken past the
/// launcher's up-front validation) fails the extraction naming the
/// file — it must never silently write an empty or partial bundle.
#[test]
fn missing_lazy_payload_fails_extraction_naming_the_file() {
    fn absent() -> Option<&'static [u8]> {
        None
    }
    let cache = tempfile::tempdir().unwrap();
    let mut bundle = test_bundle();
    bundle.push((
        PathBuf::from("w.wasm"),
        BundleContents::Lazy(LazyBytes {
            sha256: "x",
            len: 1,
            get: absent,
        }),
    ));
    let err = extract_and_lock(cache.path(), &bundle, HASH)
        .err()
        .expect("extraction must fail");
    assert!(
        format!("{err:?}").contains("w.wasm"),
        "the error names the file: {err:?}"
    );
}
