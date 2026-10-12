/*
 * stage/stages/lmd_desugar.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * lmd exploration spike: show the desugaring of `{lmd}` cells.
 */

//! `LmdDesugarStage`: rewrite every `` ```{lmd} `` cell into a plain `lmd`
//! code block whose text is the desugared Lua, so the desugaring can be
//! inspected in the rendered output (`q2 render doc.qmd --to markdown` is
//! the quickest way to look).
//!
//! This is the first spike of the lmd exploration
//! (`claude-notes/plans/2026-10-12-lmd-exploration.md`). Nothing is
//! executed: the point is to read the generated `pandoc.*` calls and judge
//! whether the desugaring could carry the real design, where a cell becomes
//! a Lua filter dispatched to the cell's position in the document.
//!
//! The stage runs before engine execution so no engine is asked to run an
//! `lmd` cell. A cell that fails to desugar (Lua syntax error, unsupported
//! markdown node) is left untouched and reported as a warning.

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::block::Block;

use crate::ast_walk::for_each_block_list_mut;
use crate::engine::capture_splice::engine_cell_lang;
use crate::lmd::{LMD_LANG, LmdError, desugar_lua};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

#[derive(Default)]
pub struct LmdDesugarStage;

impl LmdDesugarStage {
    pub fn new() -> Self {
        Self
    }
}

/// Desugar every `{lmd}` cell in `blocks` in place. Returns one error per
/// cell that was left as it was.
pub fn desugar_lmd_cells(blocks: &mut Vec<Block>) -> Vec<LmdError> {
    let mut errors = Vec::new();
    for_each_block_list_mut(blocks, &mut |list| {
        for block in list.iter_mut() {
            if engine_cell_lang(block) != Some(LMD_LANG) {
                continue;
            }
            let Block::CodeBlock(cell) = block else {
                continue;
            };
            match desugar_lua(&cell.text) {
                Ok(lua) => {
                    cell.text = lua;
                    let tag = format!("{{{LMD_LANG}}}");
                    for class in cell.attr.1.iter_mut() {
                        if *class == tag {
                            *class = LMD_LANG.to_string();
                        }
                    }
                }
                Err(err) => errors.push(err),
            }
        }
    });
    errors
}

#[async_trait(?Send)]
impl PipelineStage for LmdDesugarStage {
    fn name(&self) -> &str {
        "lmd-desugar"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        let errors = desugar_lmd_cells(&mut doc.ast.blocks);
        for err in errors {
            trace_event!(ctx, EventLevel::Warn, "{}", err);
            ctx.diagnostics
                .push(DiagnosticMessage::warning(err.to_string()));
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(qmd: &str) -> Vec<Block> {
        let mut sink: Vec<u8> = Vec::new();
        let (ast, _ctx, _warnings) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .expect("test document parses");
        ast.blocks
    }

    fn code_block(block: &Block) -> &quarto_pandoc_types::block::CodeBlock {
        match block {
            Block::CodeBlock(cb) => cb,
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn lmd_cell_becomes_displayed_desugared_lua() {
        let mut blocks = parse("::: foo\n\n```{lmd}\nreturn {= **hello** =}\n```\n\n:::\n");
        let errors = desugar_lmd_cells(&mut blocks);
        assert!(errors.is_empty(), "{errors:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div");
        };
        let cell = code_block(&div.content[0]);
        assert_eq!(cell.attr.1, vec!["lmd".to_string()]);
        assert!(engine_cell_lang(&div.content[0]).is_none());
        assert!(
            cell.text.starts_with("return pandoc.Nodes({\n"),
            "{}",
            cell.text
        );
        assert!(cell.text.contains("pandoc.Para({"), "{}", cell.text);
        assert!(cell.text.contains("pandoc.Strong({"), "{}", cell.text);
        assert!(cell.text.contains("pandoc.Str(\"hello\")"), "{}", cell.text);
    }

    #[test]
    fn other_cells_and_plain_code_are_untouched() {
        let src = "```{python}\n1 + 1\n```\n\n```lua\nreturn {= x =}\n```\n";
        let mut blocks = parse(src);
        let before = blocks.clone();
        let errors = desugar_lmd_cells(&mut blocks);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(blocks, before);
    }

    #[test]
    fn a_broken_cell_is_left_alone_and_reported() {
        let mut blocks = parse("```{lmd}\nreturn {= a =} +\n```\n");
        let before = blocks.clone();
        let errors = desugar_lmd_cells(&mut blocks);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(matches!(errors[0], LmdError::LuaParse { .. }));
        assert_eq!(blocks, before);
    }
}
