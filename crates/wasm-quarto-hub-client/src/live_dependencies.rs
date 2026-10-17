//! Translate a completed core project graph into the live preview's file
//! invalidation snapshot. No parsing, filesystem walks, or separate source
//! classifier: `ProjectIndex` is the Pass-1 snapshot the renderer just used.
//!
//! This module is only a boundary adapter. Render scheduling and all graph
//! semantics stay in quarto-core. Future element-level invalidation can
//! attach smaller render-unit keys without changing the source identity
//! contract or teaching the browser about QMD shortcodes.

use std::path::{Component, Path};

use quarto_core::project::dependency_graph::ProjectDependencyGraph;
use quarto_core::project::index::ProjectIndex;
use quarto_pandoc_types::ConfigValue;

/// Complete project-relative paths that may affect the rendered page.
///
/// `None` means UNKNOWN, *not* an empty set. Consumers must fail open and
/// re-render on all file changes when any input cannot be represented.
/// Pass-1 failures are checked by the caller before invoking this function.
/// A successful complete snapshot is safe only until the next relevant edit.
pub(crate) fn render_input_paths(
    index: &ProjectIndex,
    project_metadata: &ConfigValue,
    root: &Path,
    active_file: &Path,
) -> Option<Vec<String>> {
    let active_relative = active_file.strip_prefix(root).ok()?;
    if index.lookup_by_source(active_relative).is_none() {
        return None;
    }

    // Query core's shared profile/sidebars directly, without materializing
    // quadratic sidebar cliques. An incomplete graph still fails open.
    let mut diagnostics = Vec::new();
    let inputs = ProjectDependencyGraph::render_inputs_for_active(
        index,
        project_metadata,
        &mut diagnostics,
        active_relative,
    );
    if !diagnostics.is_empty() {
        return None; // incomplete sidebar/config resolution: don't filter
    }

    let mut paths = inputs
        .into_iter()
        .map(|input| path_for_sync(&input, root))
        .collect::<Option<Vec<_>>>()?;
    paths.sort();
    Some(paths)
}

/// Sync and WASM VFS use project-relative slash-separated paths.
/// Never strip or silently normalize '..': an out-of-tree or ambiguous
/// alias must cause broad invalidation, not a false negative.
fn path_for_sync(input: &Path, root: &Path) -> Option<String> {
    // On wasm32, VFS-rooted paths may not satisfy is_absolute().
    // Strip the known root first, then reject any unmatched root below.
    let relative = input.strip_prefix(root).unwrap_or(input);
    if relative.as_os_str().is_empty()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    let path = relative.to_str()?.replace('\\', "/");
    // Even a lexically in-tree path can spell a different sync identity.
    // Never turn ambiguous aliases into a supposedly complete snapshot.
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use quarto_core::document_profile::{DocumentProfile, IncludeEntry};

    fn profile(name: &str, files: &[&str]) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(name),
            output_href: name.replace(".qmd", ".html"),
            format_id: "html".to_string(),
            includes: files
                .iter()
                .map(|file| IncludeEntry::new(PathBuf::from(file), b"x"))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn live_snapshot_contains_active_and_transitive_partials_not_unrelated_documents() {
        let index = ProjectIndex::new(vec![
            profile("index.qmd", &["/project/outer.qmd", "/project/leaf.qmd"]),
            profile("other.qmd", &[]),
        ]);
        assert_eq!(
            render_input_paths(
                &index,
                &ConfigValue::default(),
                Path::new("/project"),
                Path::new("/project/index.qmd"),
            ),
            Some(vec![
                "index.qmd".to_string(),
                "leaf.qmd".to_string(),
                "outer.qmd".to_string(),
            ]),
        );
    }

    #[test]
    fn sidebars_expand_without_extra_navigation_graph_materialization() {
        use quarto_pandoc_types::ConfigMapEntry;
        use quarto_source_map::SourceInfo;

        let sidebar = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "contents".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_array(
                    vec![
                        ConfigValue::new_string("index.qmd", SourceInfo::for_test()),
                        ConfigValue::new_string("about.qmd", SourceInfo::for_test()),
                    ],
                    SourceInfo::for_test(),
                ),
            }],
            SourceInfo::for_test(),
        );
        let mut meta = ConfigValue::default();
        meta.insert_path(&["website", "sidebar"], sidebar);
        let index = ProjectIndex::new(vec![
            profile("index.qmd", &[]),
            profile("about.qmd", &["/project/partials/leaf.qmd"]),
            profile("unrelated.qmd", &[]),
        ]);
        assert_eq!(
            render_input_paths(
                &index,
                &meta,
                Path::new("/project"),
                Path::new("/project/index.qmd"),
            ),
            Some(vec![
                "about.qmd".to_string(),
                "index.qmd".to_string(),
                "partials/leaf.qmd".to_string(),
            ]),
        );
    }

    #[test]
    fn unknown_or_unrepresentable_dependency_must_fail_open() {
        let index = ProjectIndex::new(vec![profile("index.qmd", &["/elsewhere/secret.qmd"])]);
        assert_eq!(
            render_input_paths(
                &index,
                &ConfigValue::default(),
                Path::new("/project"),
                Path::new("/project/index.qmd"),
            ),
            None,
        );
        assert_eq!(
            path_for_sync(Path::new("a/../b.qmd"), Path::new("/project")),
            None
        );
        assert_eq!(
            path_for_sync(Path::new("./sub/leaf.qmd"), Path::new("/project")),
            None
        );
        assert_eq!(
            path_for_sync(Path::new("sub//leaf.qmd"), Path::new("/project")),
            None
        );
        assert_eq!(
            path_for_sync(Path::new("/other/b.qmd"), Path::new("/project")),
            None
        );
        assert_eq!(
            path_for_sync(Path::new("/project/sub/b.qmd"), Path::new("/project")),
            Some("sub/b.qmd".to_string())
        );
        assert_eq!(
            render_input_paths(
                &index,
                &ConfigValue::default(),
                Path::new("/project"),
                Path::new("/project/not-indexed.qmd"),
            ),
            None,
        );
    }

    #[test]
    fn profile_edges_are_included_for_active_page() {
        let mut root = profile("index.qmd", &[]);
        root.body_link_targets = vec![PathBuf::from("about.qmd")];
        let index = ProjectIndex::new(vec![
            root,
            profile("about.qmd", &["/project/sub/leaf.qmd"]),
            profile("unrelated.qmd", &[]),
        ]);
        assert_eq!(
            render_input_paths(
                &index,
                &ConfigValue::default(),
                Path::new("/project"),
                Path::new("/project/index.qmd"),
            ),
            Some(vec![
                "about.qmd".to_string(),
                "index.qmd".to_string(),
                "sub/leaf.qmd".to_string()
            ]),
        );
    }
}
