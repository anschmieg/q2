/*
 * dialect.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Source dialects: languages that compile to Lua (bd-76ogq4ew). The trait
//! is feature-independent so the unified filter pipeline can carry a list
//! of dialects; the Lua-side registration that makes the scoped `require`
//! load them is `lua::dialect`.

use std::path::Path;

use quarto_error_reporting::DiagnosticMessage;

/// A language that compiles to Lua.
pub trait SourceDialect: Send + Sync {
    /// Short identifier, used as the Lua namespace `quarto.<name>` that
    /// receives the `compile` function. `lmd` for lmd.
    fn name(&self) -> &str;

    /// File extension the loader recognises, without the dot. `lmd` for
    /// lmd.
    fn extension(&self) -> &str;

    /// Compile `source` to plain Lua. `path` is where the source came from
    /// and is for messages; the loader sets the chunk name itself. Line
    /// numbers in the result should match the source where the dialect
    /// can arrange it, so Lua's own `path:line:` positions stay true.
    fn compile(&self, source: &str, path: &Path) -> std::result::Result<String, DialectError>;
}

/// Why a dialect source did not compile. `message` is what the Lua error
/// says; `diagnostics` are the dialect's own located reports when it has
/// them (the lmd desugarer rerooting a markdown error into the module
/// file, for example), for a host that can show them.
#[derive(Debug)]
pub struct DialectError {
    pub message: String,
    pub diagnostics: Vec<DiagnosticMessage>,
}

impl DialectError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }
}

impl std::fmt::Display for DialectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DialectError {}
