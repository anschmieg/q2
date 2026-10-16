/*
 * lmd/dialect.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! lmd as a pampa source dialect (bd-76ogq4ew): the hook that lets the
//! scoped `require` load `.lmd` modules, in lmd cells, Lua filters and
//! shortcode handlers alike.
//!
//! The desugarer stays in this crate; pampa only sees the
//! [`SourceDialect`] object. Modules are compiled in the line-preserving
//! emit mode under no anchor, so a module's Lua line numbers are the
//! file's own and Lua's `path:line:` positions need no mapping. A
//! desugar failure becomes the `require` call's error, with the module
//! path and the position inside it in the message.

use std::path::Path;
use std::sync::Arc;

use pampa::dialect::{DialectError, SourceDialect};

use super::desugar::{DesugarOptions, EmitMode, LmdError, desugar_lua_with};

/// The lmd dialect: `.lmd` files are Lua with markdown fragments.
#[derive(Debug, Default, Clone, Copy)]
pub struct LmdDialect;

impl SourceDialect for LmdDialect {
    fn name(&self) -> &str {
        super::LMD_LANG
    }

    fn extension(&self) -> &str {
        super::LMD_LANG
    }

    fn compile(&self, source: &str, path: &Path) -> Result<String, DialectError> {
        let opts = DesugarOptions {
            anchor: None,
            mode: EmitMode::LinePreserving,
        };
        desugar_lua_with(source, &opts).map_err(|err| {
            let diagnostics = match &err {
                LmdError::MarkdownParse { diagnostics, .. } => diagnostics.clone(),
                _ => Vec::new(),
            };
            DialectError {
                message: format!("{}: {err}", path.display()),
                diagnostics,
            }
        })
    }
}

/// The dialects q2 registers on every Lua state it builds: lmd.
pub fn dialects() -> Vec<Arc<dyn SourceDialect>> {
    vec![Arc::new(LmdDialect)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_a_module_keeping_its_lines() {
        let src =
            "local M = {}\nfunction M.card(t)\n  return {=: **`{lmd} t`** :=}\nend\nreturn M\n";
        let out = LmdDialect.compile(src, Path::new("cards.lmd")).unwrap();
        assert_eq!(out.lines().count(), src.lines().count(), "{out}");
        assert!(out.contains("pandoc.Blocks("), "{out}");
    }

    #[test]
    fn a_lua_syntax_error_names_the_module_and_position() {
        let err = LmdDialect
            .compile("local = 1\n", Path::new("bad.lmd"))
            .unwrap_err();
        assert!(err.message.starts_with("bad.lmd: "), "{}", err.message);
        assert!(err.message.contains("line 1"), "{}", err.message);
    }

    #[test]
    fn a_markdown_error_carries_the_readers_diagnostics() {
        let err = LmdDialect
            .compile("return {=: pages' :=}\n", Path::new("bad.lmd"))
            .unwrap_err();
        assert!(
            err.message.contains("markdown fragment failed to parse"),
            "{}",
            err.message
        );
        assert!(!err.diagnostics.is_empty());
    }

    #[test]
    fn dialects_is_lmd() {
        let ds = dialects();
        assert_eq!(ds.len(), 1);
        assert_eq!((ds[0].name(), ds[0].extension()), ("lmd", "lmd"));
    }
}
