/*
 * langs/lmd.rs
 * Copyright (c) 2026 Posit, PBC
 */

// Query source: tree_sitter_lmd::HIGHLIGHTS_QUERY (the vendored grammar's
// `queries/highlights.scm`: upstream Lua's captures plus the `{= … =}`
// fragment delimiters and content). lmd is Lua with markdown fragments
// (bd-76ogq4ew), so an echoed or failed `{lmd}` cell highlights its Lua
// and marks the fragments rather than falling back to plain text.
use tree_sitter_highlight::HighlightConfiguration;

use crate::error::HighlightError;
use crate::langs::build_for;

pub(crate) fn build() -> Result<(HighlightConfiguration, Vec<String>), HighlightError> {
    build_for(
        tree_sitter_lmd::LANGUAGE.into(),
        "lmd",
        tree_sitter_lmd::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}
