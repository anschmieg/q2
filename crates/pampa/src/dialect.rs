/*
 * dialect.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Source dialects: languages that compile to Lua (bd-76ogq4ew). The trait
//! is feature-independent so the unified filter pipeline can carry a list
//! of dialects; the Lua-side registration that makes the scoped `require`
//! load them is `lua::dialect`.

use std::path::{Path, PathBuf};

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

/// A module the scoped `require` found as dialect source but could not
/// compile. The loader raises it as the `require` call's error (through
/// `mlua::Error::external`), so a host that catches the cell's or
/// filter's failure can recognise it in the error chain and report it
/// under its own code, with the dialect's diagnostics attached.
#[derive(Debug)]
pub struct ModuleCompileError {
    /// The name as written in `require`.
    pub module: String,
    /// The file that was found.
    pub path: PathBuf,
    /// The dialect's name (`lmd`).
    pub dialect: String,
    pub error: DialectError,
}

impl std::fmt::Display for ModuleCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "module '{}' could not be compiled from {} ({}): {}",
            self.module,
            self.dialect,
            self.path.display(),
            self.error.message
        )
    }
}

impl std::error::Error for ModuleCompileError {}

impl ModuleCompileError {
    /// Find a `ModuleCompileError` in an mlua error, however many callback
    /// and context layers Lua's unwinding wrapped around it.
    pub fn find_in(err: &mlua_error::Error) -> Option<&ModuleCompileError> {
        mlua_error::find_module_compile_error(err)
    }
}

/// The mlua-dependent half of [`ModuleCompileError::find_in`], kept behind
/// the Lua feature so the type itself stays feature-independent.
#[cfg(feature = "lua-filter")]
mod mlua_error {
    pub use mlua::Error;

    pub fn find_module_compile_error(err: &Error) -> Option<&super::ModuleCompileError> {
        match err {
            Error::ExternalError(e) => e.downcast_ref::<super::ModuleCompileError>(),
            Error::CallbackError { cause, .. } => find_module_compile_error(cause),
            Error::WithContext { cause, .. } => find_module_compile_error(cause),
            _ => None,
        }
    }
}
