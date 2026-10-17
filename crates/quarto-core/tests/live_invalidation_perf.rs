//! Informational Q2-M1 dependency-graph performance baseline.
//! Synthetic Pass-1 profiles isolate graph build and closure cost from parsing,
//! sync, WASM serialization, and browser latency. CI timing is not an assertion.

use std::path::{Path, PathBuf};
use std::time::Instant;

use quarto_core::document_profile::{DocumentProfile, IncludeEntry};
use quarto_core::glob::GlobPattern;
use quarto_core::project::dependency_graph::ProjectDependencyGraph;
use quarto_core::project::index::ProjectIndex;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
use quarto_source_map::SourceInfo;

fn profile(name: &str, includes: usize) -> DocumentProfile {
    DocumentProfile {
        source_path: PathBuf::from(name),
        output_href: name.replace(".qmd", ".html"),
        format_id: "html".to_string(),
        includes: (0..includes)
            .map(|i| {
                IncludeEntry::new(
                    PathBuf::from(format!("/project/partials/{i:04}.qmd")),
                    b"x",
                )
            })
            .collect(),
        ..DocumentProfile::default()
    }
}

fn baseline(label: &str, index: &ProjectIndex, meta: &ConfigValue, expected_inputs: usize) {
    let mut builds = Vec::new();
    let mut queries = Vec::new();
    let mut estimated_bytes = 0;
    for _ in 0..5 {
        let mut diagnostics = Vec::new();
        let start = Instant::now();
        let graph = ProjectDependencyGraph::build(index, meta, &mut diagnostics);
        builds.push(start.elapsed().as_micros());
        assert!(diagnostics.is_empty(), "{label}: incomplete graph");

        let start = Instant::now();
        let inputs = graph.render_inputs_for(Path::new("active.qmd"));
        queries.push(start.elapsed().as_micros());
        assert_eq!(inputs.len(), expected_inputs, "{label}: graph closure");
        estimated_bytes = inputs
            .iter()
            .map(|p| p.to_string_lossy().len() + 3)
            .sum();
    }
    builds.sort_unstable();
    queries.sort_unstable();
    println!(
        "q2_m1_perf scenario={label} documents={} inputs={} build_us_p50={} query_us_p50={} estimated_json_bytes={estimated_bytes}",
        index.len(),
        expected_inputs,
        builds[builds.len() / 2],
        queries[queries.len() / 2],
    );
}

#[test]
fn baseline_flat_projects_and_transitive_includes() {
    for documents in [100, 1000, 3000] {
        for includes in [0, 1, 8, 64] {
            let mut profiles = vec![profile("active.qmd", includes)];
            for i in 1..documents {
                profiles.push(profile(&format!("unrelated-{i:04}.qmd"), 0));
            }
            baseline(
                &format!("flat_{documents}_includes_{includes}"),
                &ProjectIndex::new(profiles),
                &ConfigValue::default(),
                includes + 1,
            );
        }
    }
}

#[test]
fn baseline_large_sidebars() {
    for documents in [32, 128, 256, 512] {
        let names: Vec<String> = (0..documents)
            .map(|i| {
                if i == 0 {
                    "active.qmd".to_string()
                } else {
                    format!("page-{i:04}.qmd")
                }
            })
            .collect();
        let index = ProjectIndex::new(names.iter().map(|name| profile(name, 0)).collect());
        let contents = ConfigValue::new_array(
            names
                .iter()
                .map(|name| ConfigValue::new_string(name.as_str(), SourceInfo::for_test()))
                .collect(),
            SourceInfo::for_test(),
        );
        let sidebar = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "contents".to_string(),
                key_source: SourceInfo::for_test(),
                value: contents,
            }],
            SourceInfo::for_test(),
        );
        let mut meta = ConfigValue::default();
        meta.insert_path(&["website", "sidebar"], sidebar);
        baseline(&format!("sidebar_{documents}"), &index, &meta, documents);
    }
}

#[test]
fn baseline_listing_pattern_expansion() {
    for globs in [1, 32] {
        let mut active = profile("active.qmd", 0);
        active.listing_content_globs = (0..globs)
            .map(|i| {
                if globs == 1 {
                    GlobPattern::positive("posts/*/*.qmd")
                } else {
                    GlobPattern::positive(format!("posts/section-{i:02}/*.qmd"))
                }
            })
            .collect();
        let mut profiles = vec![active];
        for i in 0..1000 {
            profiles.push(profile(
                &format!("posts/section-{:02}/post-{i:04}.qmd", i % globs),
                0,
            ));
        }
        baseline(
            &format!("listing_1000_globs_{globs}"),
            &ProjectIndex::new(profiles),
            &ConfigValue::default(),
            1001,
        );
    }
}
