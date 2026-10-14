use pagefind::api::PagefindIndex;
use serde::Deserialize;
use std::collections::BTreeMap as HashMap;

#[derive(Deserialize)]
struct Rec { href: String, title: String, section: String, text: String }

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let json = std::fs::read_to_string(&args[1]).unwrap();
    let recs: Vec<Rec> = serde_json::from_str(&json).unwrap();
    let mode = args.get(3).map(|s| s.as_str()).unwrap_or("html");
    let mut idx = PagefindIndex::new(None).unwrap();
    // group sections by page so each page is one pagefind record with headings
    let mut pages: Vec<(String, String, Vec<(String, String, String)>)> = vec![];
    for r in recs {
        let (page, anchor) = match r.href.split_once('#') { Some((p, a)) => (p.to_string(), a.to_string()), None => (r.href.clone(), String::new()) };
        if let Some(p) = pages.iter_mut().find(|p| p.0 == page) { p.2.push((anchor, r.section, r.text)); }
        else { pages.push((page, r.title, vec![(anchor, r.section, r.text)])); }
    }
    let t0 = std::time::Instant::now();
    for (page, title, secs) in &pages {
        if mode == "html" {
            let mut html = format!("<html><body><main><h1>{}</h1>", html_escape(title));
            for (anchor, section, text) in secs {
                if !section.is_empty() { html.push_str(&format!("<h2 id=\"{}\">{}</h2>", anchor, html_escape(section))); }
                html.push_str(&format!("<p>{}</p>", html_escape(text)));
            }
            html.push_str("</main></body></html>");
            idx.add_html_file(None, Some(format!("/{}", page)), html).await.unwrap();
        } else {
            let text: String = secs.iter().map(|s| format!("{} {}", s.1, s.2)).collect::<Vec<_>>().join("\n");
            let mut meta = HashMap::new(); meta.insert("title".to_string(), title.clone());
            idx.add_custom_record(format!("/{}", page), text, "en".into(), Some(meta), None, None).await.unwrap();
        }
    }
    let t1 = t0.elapsed();
    idx.build_indexes().await.unwrap();
    let t2 = t0.elapsed();
    let files = idx.get_files().await.unwrap();
    let mut by_kind: std::collections::BTreeMap<String, (usize, u64)> = Default::default();
    for f in &files {
        let p = f.filename.to_string_lossy().to_string();
        let kind = if p.contains("fragment/") { "fragment" } else if p.contains("index/") { "index" } else if p.contains("filter/") { "filter" } else if p.ends_with(".pf_meta") { "meta" } else if p.contains("wasm") { "wasm" } else { "other" };
        let e = by_kind.entry(kind.to_string()).or_default(); e.0 += 1; e.1 += f.contents.len() as u64;
    }
    println!("mode={mode} pages={} add={:?} build={:?}", pages.len(), t1, t2 - t1);
    for (k, (n, b)) in by_kind { println!("  {k:9} files={n:5} bytes={b}"); }
    if let Some(out) = args.get(2) { idx.write_files(Some(out.into())).await.unwrap(); }
}
fn html_escape(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") }
