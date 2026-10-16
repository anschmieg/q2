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
//! desugar failure becomes the `require` call's error; the loader names
//! the module and its path, the dialect's message gives the position inside
//! the file and what went wrong.

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
            // The loader (and `quarto.lmd.compile`) prefix the module path
            // and name; the message carries the position inside the file.
            let _ = path;
            DialectError {
                message: err.to_string(),
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
        assert!(err.message.contains("Lua syntax error"), "{}", err.message);
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

    const CARDS_LMD: &str = "local M = {}\nfunction M.card(title, body)\n  return {=:\n::: {.card}\n### `{lmd} title`\n\n`{lmd} body`\n:::\n:=}\nend\nreturn M\n";

    fn write(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn one_paragraph_doc() -> pampa::pandoc::Pandoc {
        use pampa::pandoc::{Block, Inline, Pandoc, Paragraph, Str};
        Pandoc {
            meta: quarto_pandoc_types::ConfigValue::default(),
            blocks: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::for_test(),
                })],
                source_info: quarto_source_map::SourceInfo::for_test(),
            })],
        }
    }

    /// A Lua *filter* (not a cell) can `require` an `.lmd` module when the
    /// filter pipeline is given q2's dialects, which is how
    /// `UserFiltersStage` calls it.
    #[test]
    fn a_lua_filter_can_require_an_lmd_module() {
        use pampa::pandoc::Block;
        use pampa::unified_filter::{FilterSpec, apply_filters_with};
        let tmp = tempfile::TempDir::new().unwrap();
        write(tmp.path(), "components/cards.lmd", CARDS_LMD);
        write(
            tmp.path(),
            "filter.lua",
            "local cards = require(\"components/cards\")\nfunction Para(p)\n  return cards.card(\"From a filter\", p.content)\nend\n",
        );
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let output = pollster::block_on(apply_filters_with(
            one_paragraph_doc(),
            pampa::pandoc::ASTContext::new(),
            &[FilterSpec::Lua(tmp.path().join("filter.lua"))],
            "html",
            runtime,
            None,
            tmp.path(),
            &dialects(),
        ))
        .expect("filter runs");
        assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
        let Block::Div(div) = &output.pandoc.blocks[0] else {
            panic!("expected the card div, got {:?}", output.pandoc.blocks[0]);
        };
        assert_eq!(div.attr.1, vec!["card".to_string()]);
    }

    /// A shortcode handler script can `require` an `.lmd` module when the
    /// engine is built with q2's dialects, as `ShortcodeResolveTransform`
    /// builds it.
    #[test]
    fn a_shortcode_handler_can_require_an_lmd_module() {
        use pampa::lua::{
            LuaShortcodeEngine, LuaShortcodeResult, ShortcodeArgs, ShortcodeCallContext,
        };
        let tmp = tempfile::TempDir::new().unwrap();
        write(tmp.path(), "components/cards.lmd", CARDS_LMD);
        write(
            tmp.path(),
            "cards-shortcode.lua",
            "local cards = require(\"components/cards\")\nreturn {\n  card = function(args, kwargs, meta, raw_args, context)\n    return cards.card(args[1], args[2])\n  end\n}\n",
        );
        let runtime: Arc<dyn quarto_system_runtime::SystemRuntime> =
            Arc::new(quarto_system_runtime::NativeRuntime::new());
        let mut engine = LuaShortcodeEngine::new_with("html", runtime, &dialects()).unwrap();
        pollster::block_on(engine.load_script(&tmp.path().join("cards-shortcode.lua"))).unwrap();
        let args = ShortcodeArgs {
            positional: vec!["Title".to_string(), "body".to_string()],
            keyword: vec![],
            metadata: quarto_pandoc_types::ConfigValue::default(),
        };
        let result = pollster::block_on(engine.call("card", &args, ShortcodeCallContext::Block))
            .expect("handler registered");
        match result {
            LuaShortcodeResult::Blocks(blocks) => {
                assert!(
                    matches!(blocks[0], pampa::pandoc::Block::Div(_)),
                    "{blocks:?}"
                );
            }
            other => panic!("expected blocks, got {other:?}"),
        }
    }

    #[test]
    fn dialects_is_lmd() {
        let ds = dialects();
        assert_eq!(ds.len(), 1);
        assert_eq!((ds[0].name(), ds[0].extension()), ("lmd", "lmd"));
    }
}
