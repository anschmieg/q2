//! CI-only optimized-WASM benchmark of the exact core live-invalidation
//! entry point. Built with `--features m1-benchmark`; ordinary production
//! builds do not expose this function or link its synthetic fixtures.

use std::path::{Path, PathBuf};

use quarto_core::document_profile::DocumentProfile;
use quarto_core::project::dependency_graph::ProjectDependencyGraph;
use quarto_core::project::index::ProjectIndex;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::{By, SourceInfo};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance)]
    fn now() -> f64;
}

fn origin() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

/// Run a median of `iterations` graph constructions and queries in the
/// *already optimized* WASM module. One sidebar holds `pages` distinct
/// profiles, just as the native baseline does. Return an informational JSON
/// record; never compare wall-clock results against a hard CI threshold.
#[wasm_bindgen]
pub fn benchmark_live_sidebar_query(pages: usize, iterations: usize) -> String {
    assert!((2..=2048).contains(&pages));
    assert!((1..=100).contains(&iterations));

    let names: Vec<String> = (0..pages)
        .map(|i| {
            if i == 0 {
                "active.qmd".to_string()
            } else {
                format!("page-{i:04}.qmd")
            }
        })
        .collect();
    let profiles = names
        .iter()
        .map(|name| DocumentProfile {
            source_path: PathBuf::from(name),
            output_href: name.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            ..DocumentProfile::default()
        })
        .collect();
    let index = ProjectIndex::new(profiles);
    let sidebar = ConfigValue::new_map(
        vec![ConfigMapEntry {
            key: "contents".to_string(),
            key_source: origin(),
            value: ConfigValue::new_array(
                names
                    .iter()
                    .map(|name| ConfigValue::new_string(name.as_str(), origin()))
                    .collect(),
                origin(),
            ),
        }],
        origin(),
    );
    let mut metadata = ConfigValue::default();
    metadata.insert_path(&["website", "sidebar"], sidebar);

    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let mut diagnostics = Vec::new();
        let start = now();
        let inputs = ProjectDependencyGraph::render_inputs_for_active(
            &index,
            &metadata,
            &mut diagnostics,
            Path::new("active.qmd"),
        );
        samples.push(now() - start);
        assert!(diagnostics.is_empty());
        assert_eq!(inputs.len(), pages);
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    serde_json::json!({
        "pages": pages,
        "inputs": pages,
        "iterations": iterations,
        "optimized_wasm_ms_p50": samples[samples.len() / 2],
        "optimized_wasm_ms_min": samples[0],
        "optimized_wasm_ms_max": samples[samples.len() - 1],
    })
    .to_string()
}
