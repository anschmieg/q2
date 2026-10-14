//! Phase 1 tests (bd-ee0qcq3c; plan
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`):
//! precompressed `.gz` serving and cache headers for the embedded-SPA
//! asset path. Landed as ignored skeletons in Phase 0 (named `br_*`);
//! un-ignored and made green in Phase 1 under the gz-only decision
//! (see the plan's communication record).
//!
//! Contracts pinned here:
//!
//! - `Accept-Encoding: gzip` on a precompressible asset →
//!   `Content-Encoding: gzip` + `Vary: Accept-Encoding` + the `.gz`
//!   sibling bytes; Content-Type unchanged.
//! - No `gzip` in `Accept-Encoding` (including a `br`-only client) →
//!   identity bytes, no `Content-Encoding`. (Identity stays embedded
//!   for clients that don't send `gzip`.)
//! - The `.gz` bytes gunzip to the identity bytes.
//! - Cache headers match the local-prod contract
//!   (`scripts/local-prod-server.mjs`): paths under `/assets/` (Vite
//!   content-hashed) get `public, max-age=31536000, immutable`;
//!   everything else (e.g. `/`) gets `no-cache`.
//!
//! Placeholder trees (fresh clone, no built dist) have no `/assets/*`
//! files to fetch; the tests no-op there, matching the crate's
//! both-tree-states embed-test pattern.

use axum::Router;
use quarto_preview::extend_with_spa;

async fn spawn_with_addr(app: Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 127.0.0.1:0");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

/// Fetch `/` and extract the first referenced `/assets/<name>-<hash>.<ext>`
/// path (the dist's content-hashed filenames change every build, so the
/// test discovers one instead of hardcoding). `None` on a placeholder
/// tree (nothing to compress there).
async fn discover_asset_path(addr: std::net::SocketAddr) -> Option<String> {
    let index = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("GET /")
        .error_for_status()
        .expect("200 OK")
        .text()
        .await
        .expect("index body");
    extract_asset_path(&index)
}

/// Find the first `assets/<name>-<hash>.<ext>` reference in the index
/// HTML (references look like `./assets/main-D3X1slXQ.js`). Hand-rolled
/// to avoid a regex dev-dep for one scan.
fn extract_asset_path(html: &str) -> Option<String> {
    let start = html.find("assets/")?;
    let rest = &html[start..];
    let end = rest.find(['"', '\''])?;
    let candidate = &rest[..end];
    match candidate.rsplit('.').next() {
        Some("js" | "css" | "wasm") => Some(format!("/{candidate}")),
        _ => None,
    }
}

/// GET `path` with an explicit `Accept-Encoding`; returns (response
/// headers, raw body bytes). reqwest is built without compression
/// features, so it neither sends `Accept-Encoding` on its own nor
/// transparently decompresses — the raw wire behavior is what the
/// assertions see.
async fn get_with_encoding(
    addr: std::net::SocketAddr,
    path: &str,
    accept_encoding: Option<&str>,
) -> (reqwest::header::HeaderMap, Vec<u8>) {
    let client = reqwest::Client::new();
    let mut req = client.get(format!("http://{addr}{path}"));
    if let Some(enc) = accept_encoding {
        req = req.header(reqwest::header::ACCEPT_ENCODING, enc);
    }
    let resp = req.send().await.expect("GET asset");
    assert_eq!(resp.status(), 200, "asset must serve 200");
    let headers = resp.headers().clone();
    let body = resp.bytes().await.expect("body bytes").to_vec();
    (headers, body)
}

#[tokio::test]
async fn gz_served_when_accepted() {
    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;
    let Some(asset) = discover_asset_path(addr).await else {
        eprintln!("placeholder tree (no built dist); nothing to compress");
        return;
    };

    let (headers, body) = get_with_encoding(addr, &asset, Some("gzip")).await;
    assert_eq!(
        headers
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok()),
        Some("gzip"),
        "Accept-Encoding: gzip must yield Content-Encoding: gzip; headers: {headers:?}"
    );
    let vary = headers
        .get(reqwest::header::VARY)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        vary.split(',')
            .any(|t| t.trim().eq_ignore_ascii_case("accept-encoding")),
        "encoded responses must carry Vary: Accept-Encoding; got Vary: {vary:?}"
    );
    // Content-Type describes the *decoded* entity.
    let expected_ct = if asset.ends_with(".js") {
        "application/javascript"
    } else if asset.ends_with(".css") {
        "text/css"
    } else {
        "application/wasm"
    };
    assert_eq!(
        headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some(expected_ct),
        "Content-Type must survive content negotiation"
    );
    if let Some(cl) = headers.get(reqwest::header::CONTENT_LENGTH) {
        assert_eq!(
            cl.to_str().unwrap().parse::<usize>().unwrap(),
            body.len(),
            "Content-Length must describe the encoded (transport) body"
        );
    }
}

#[tokio::test]
async fn identity_served_without_gz_acceptance() {
    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;
    let Some(asset) = discover_asset_path(addr).await else {
        eprintln!("placeholder tree (no built dist); nothing to compress");
        return;
    };

    // No header at all, and a brotli-only client: both get identity.
    for accept in [None, Some("br")] {
        let (headers, body) = get_with_encoding(addr, &asset, accept).await;
        assert!(
            headers.get(reqwest::header::CONTENT_ENCODING).is_none(),
            "Accept-Encoding {accept:?} must yield identity (no Content-Encoding); headers: {headers:?}"
        );
        assert!(
            !body.is_empty(),
            "identity body must be the embedded asset bytes"
        );
    }
}

#[tokio::test]
async fn gz_bytes_roundtrip_to_identity() {
    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;
    let Some(asset) = discover_asset_path(addr).await else {
        eprintln!("placeholder tree (no built dist); nothing to compress");
        return;
    };

    let (_, gz_body) = get_with_encoding(addr, &asset, Some("gzip")).await;
    let (_, identity) = get_with_encoding(addr, &asset, None).await;

    assert!(
        gz_body.len() < identity.len(),
        "gzip should shrink the asset ({} !< {})",
        gz_body.len(),
        identity.len()
    );
    let mut decoded = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::GzDecoder::new(gz_body.as_slice()),
        &mut decoded,
    )
    .expect("gz body gunzips");
    assert_eq!(
        decoded, identity,
        "decompressed .gz bytes must equal the identity bytes"
    );
}

#[tokio::test]
async fn cache_headers_match_local_prod_contract() {
    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;

    // `/` (the SPA index) must always revalidate.
    let index = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("GET /")
        .error_for_status()
        .expect("200 OK");
    let cc = index
        .headers()
        .get(reqwest::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        cc, "no-cache",
        "the SPA index must always revalidate (local-prod contract); got Cache-Control: {cc:?}"
    );
    let index_body = index.text().await.expect("index body");

    // Content-hashed `/assets/*` are immutable.
    let Some(asset) = extract_asset_path(&index_body) else {
        eprintln!("placeholder tree (no built dist); no /assets/* to check");
        return;
    };
    let (headers, _) = get_with_encoding(addr, &asset, None).await;
    let cc = headers
        .get(reqwest::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        cc, "public, max-age=31536000, immutable",
        "content-hashed assets are immutable (local-prod contract); got Cache-Control: {cc:?}"
    );
}

/// The render-WASM dedupe contract (plan
/// `claude-notes/plans/2026-10-13-wasm-embed-dedupe.md`): the ~36 MB
/// hashed wasm is stripped from the viewer archive and served from the
/// shared `quarto-wasm-embed` crate, so the q2 binary carries it once.
/// Pin that the served bytes ARE the shared embed's bytes and that
/// they satisfy the npm-written spa-manifest.json entry (a `--join`
/// guest in local-asset mode routes on and trusts the manifest).
#[tokio::test]
async fn wasm_served_from_shared_embed_matches_manifest() {
    use sha2::Digest as _;

    let addr = spawn_with_addr(extend_with_spa(Router::new())).await;

    // The manifest rides the archive at the dist root. A placeholder
    // tree has none — the SPA fallback answers index.html (text/html)
    // instead; skip there, matching this file's both-tree-states
    // pattern.
    let resp = reqwest::get(format!("http://{addr}/spa-manifest.json"))
        .await
        .expect("GET /spa-manifest.json");
    let is_json = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/json"));
    if !is_json {
        eprintln!("placeholder tree (no built dist); no manifest to check");
        return;
    }
    let manifest: serde_json::Value = resp.json().await.expect("manifest parses");
    let entry = manifest["entries"]
        .as_array()
        .expect("manifest entries")
        .iter()
        .find(|e| {
            e["path"].as_str().is_some_and(|p| {
                p.starts_with("assets/wasm_quarto_hub_client_bg-") && p.ends_with(".wasm")
            })
        })
        .expect("the manifest lists the hashed render wasm");
    let rel = entry["path"].as_str().unwrap();
    let manifest_sha = entry["sha256"].as_str().unwrap();
    let manifest_size = entry["size"].as_u64().unwrap();

    let shared = quarto_wasm_embed::wasm_bytes()
        .expect("a manifest-bearing tree implies the shared crate embedded the wasm");

    // Identity: the shared embed's bytes, as application/wasm.
    let (headers, identity) = get_with_encoding(addr, &format!("/{rel}"), None).await;
    assert_eq!(
        identity, shared,
        "served wasm must be the shared embed's bytes"
    );
    assert_eq!(
        identity.len() as u64,
        manifest_size,
        "manifest size must describe the served bytes"
    );
    let digest = sha2::Sha256::digest(&identity)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        digest, manifest_sha,
        "manifest sha256 must describe the served bytes"
    );
    assert_eq!(
        headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/wasm"),
        "wasm must serve as application/wasm"
    );

    // The lazily generated gzip variant decodes to the same bytes.
    let (gz_headers, gz_body) = get_with_encoding(addr, &format!("/{rel}"), Some("gzip")).await;
    assert_eq!(
        gz_headers
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok()),
        Some("gzip"),
        "the wasm is gzipped at runtime (the .gz sibling is never embedded)"
    );
    let mut decoded = Vec::new();
    std::io::Read::read_to_end(
        &mut flate2::read::GzDecoder::new(gz_body.as_slice()),
        &mut decoded,
    )
    .expect("gz body gunzips");
    assert_eq!(
        decoded, identity,
        "gunzipped wasm must equal the identity bytes"
    );
}
