/*
 * tests/integration/components_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end pins for components bound to document elements
 * (bd-react-components-a8flgldm): the static page, the shipped assets,
 * and the no-script invariants.
 */

//! Renders the `components-basic` website fixture with the real binary and
//! checks what the plan's S1 promises:
//!
//! - the bound div carries `data-quarto-component` and its content is the
//!   markdown as written (the no-script version);
//! - the head carries the import map, the manifest and the runtime module
//!   tag, in that order, with page-relative URLs;
//! - the component module, the runtime and the vendored React modules land
//!   under `site_libs/`, byte-identical to their sources;
//! - a page without components ships none of it, and `--to markdown` sees
//!   no binding at all.
//!
//! What a browser does with all this is checked by the plan's Chromium
//! smoke (the `quarto-component:mounted` event and the click), not here.

use std::path::{Path, PathBuf};
use std::process::Command;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration/fixtures/components-basic")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

struct Rendered {
    dir: tempfile::TempDir,
    stderr: String,
}

impl Rendered {
    fn site(&self) -> PathBuf {
        self.dir.path().join("_site")
    }
    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.site().join(rel))
            .unwrap_or_else(|e| panic!("{rel}: {e}\nstderr:\n{}", self.stderr))
    }
}

fn render(extra_args: &[&str]) -> Rendered {
    let dir = tempfile::TempDir::new().unwrap();
    copy_dir(&fixture(), dir.path());
    let output = Command::new(Q2_BIN)
        .arg("render")
        .args(extra_args)
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("q2 runs");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "render failed: {:?}\nstdout:\n{}\nstderr:\n{stderr}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
    Rendered { dir, stderr }
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> &'a str {
    let a = s
        .find(start)
        .unwrap_or_else(|| panic!("missing {start:?} in:\n{s}"));
    let b = match s[a..].find(end) {
        Some(i) => a + i + end.len(),
        None => panic!("missing {end:?}"),
    };
    &s[a..b]
}

#[test]
fn bound_div_keeps_its_markup_and_gains_the_host_attribute() {
    let r = render(&[]);
    assert!(
        !r.stderr.contains("Q-25-"),
        "no component diagnostics expected:\n{}",
        r.stderr
    );
    let html = r.read("index.html");
    let div = between(&html, "<div id=\"counter\"", "</div>");
    assert!(
        div.starts_with("<div id=\"counter\" class=\"demo\" data-quarto-component=\"counter\">"),
        "host attribute on the element as written: {div}"
    );
    assert!(
        div.contains("<strong>no-script</strong> content: a count of <code>0</code>"),
        "the element's own content is the markdown rendering: {div}"
    );
}

#[test]
fn head_carries_import_map_manifest_and_runtime_in_order() {
    let r = render(&[]);
    let html = r.read("index.html");
    let head = between(&html, "<head>", "</head>");
    let import_map = head
        .find("<script type=\"importmap\">")
        .expect("import map");
    let manifest = head
        .find("<script type=\"application/json\" id=\"quarto-components-manifest\">")
        .expect("manifest");
    let runtime = head
        .find("<script type=\"module\" src=\"site_libs/quarto-components/quarto-components.js\"></script>")
        .expect("runtime module tag");
    assert!(
        import_map < manifest && manifest < runtime,
        "order in head:\n{head}"
    );
    // The import map must precede every module script on the page.
    let first_module = head.find("type=\"module\"").unwrap();
    assert!(
        import_map < first_module,
        "import map after a module script:\n{head}"
    );
    assert!(
        head.contains("\"react\":\"./site_libs/quarto-react/react.js\""),
        "{head}"
    );
    assert!(head.contains("\"react-dom/client\":\"./site_libs/quarto-react/react-dom-client.js\""));
    assert!(
        head.contains("\"react/jsx-runtime\":\"./site_libs/quarto-react/react-jsx-runtime.js\"")
    );
    assert!(
        head.contains("\"counter\":{\"content\":\"dom\",\"export\":\"default\",\"module\":\"site_libs/quarto-components/counter.js\"}"),
        "{head}"
    );
}

#[test]
fn modules_runtime_and_react_land_under_site_libs_verbatim() {
    let r = render(&[]);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pairs = [
        (
            "site_libs/quarto-components/counter.js",
            fixture().join("counter.js"),
        ),
        (
            "site_libs/quarto-components/quarto-components.js",
            repo.join("resources/js/quarto-components/quarto-components.js"),
        ),
        (
            "site_libs/quarto-react/react.js",
            repo.join("resources/js/react/react.js"),
        ),
        (
            "site_libs/quarto-react/react-jsx-runtime.js",
            repo.join("resources/js/react/react-jsx-runtime.js"),
        ),
        (
            "site_libs/quarto-react/react-dom.js",
            repo.join("resources/js/react/react-dom.js"),
        ),
        (
            "site_libs/quarto-react/react-dom-client.js",
            repo.join("resources/js/react/react-dom-client.js"),
        ),
    ];
    for (rel, source) in pairs {
        let shipped = r.read(rel);
        let expected = std::fs::read_to_string(&source).unwrap();
        assert_eq!(
            shipped,
            expected,
            "{rel} must be byte-identical to {}",
            source.display()
        );
    }
}

#[test]
fn markdown_output_sees_no_binding() {
    // The transform is HTML-render only: the pandoc leg leaves the div
    // exactly as the author wrote it.
    let dir = tempfile::TempDir::new().unwrap();
    copy_dir(&fixture(), dir.path());
    let output = Command::new(Q2_BIN)
        .args(["render", "index.qmd", "--to", "markdown"])
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("q2 runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let md = std::fs::read_to_string(dir.path().join("_site/index.md")).unwrap();
    assert!(md.contains("::: {#counter .demo}"), "{md}");
    assert!(!md.contains("quarto-component"), "{md}");
}

#[test]
fn page_without_components_ships_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    copy_dir(&fixture(), dir.path());
    std::fs::write(
        dir.path().join("index.qmd"),
        "---\ntitle: plain\n---\n\n::: {#counter}\nJust a div.\n:::\n",
    )
    .unwrap();
    let output = Command::new(Q2_BIN)
        .arg("render")
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("q2 runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let html = std::fs::read_to_string(dir.path().join("_site/index.html")).unwrap();
    assert!(!html.contains("quarto-component"), "{html}");
    assert!(!html.contains("importmap"), "{html}");
    assert!(!dir.path().join("_site/site_libs/quarto-react").exists());
    assert!(
        !dir.path()
            .join("_site/site_libs/quarto-components")
            .exists()
    );
}

#[test]
fn unbound_component_warns_q_25_4_and_ships_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    copy_dir(&fixture(), dir.path());
    std::fs::write(
        dir.path().join("index.qmd"),
        "---\ntitle: plain\ncomponents:\n  - name: counter\n    src: counter.js\n---\n\nNo host here.\n",
    )
    .unwrap();
    let output = Command::new(Q2_BIN)
        .arg("render")
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("q2 runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("Q-25-4"), "{stderr}");
    assert!(stderr.contains("`#counter`"), "{stderr}");
    assert!(
        !dir.path()
            .join("_site/site_libs/quarto-components")
            .exists()
    );
}
