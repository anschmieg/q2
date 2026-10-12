/*
 * stage/stages/lmd_desugar.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * lmd exploration spike: show the desugaring of `{lmd}` cells.
 */

//! `LmdDesugarStage`: rewrite every `` ```{lmd} `` cell in place into a
//! plain `lmd` code block whose text is the desugared Lua, marked
//! `lmd="pending"` for [`super::LmdExecuteStage`] to pick up after the
//! engine has run.
//!
//! The stage runs before engine execution so no engine is asked to run an
//! `lmd` cell: an unbraced `lmd` class is plain code to every engine. A
//! cell that fails to desugar (Lua syntax error, unsupported markdown node)
//! is left untouched and reported as a warning. Spike 1 of
//! `claude-notes/plans/2026-10-12-lmd-exploration.md` stopped here, which
//! is why `q2 render --to markdown` of a document shows the desugared Lua
//! whenever the execute stage is absent or a cell fails to run.

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_source_map::SourceInfo;

use crate::ast_walk::{for_each_block_list_mut, for_each_inline_list_mut};
use crate::engine::capture_splice::engine_cell_lang;
use crate::lmd::{
    LMD_LANG, LMD_PENDING_ATTR, LMD_PENDING_VALUE, LmdError, desugar_lua, desugar_lua_expr,
    inline_lmd_source,
};
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

/// A cell that could not be desugared and was left as written.
#[derive(Debug)]
pub struct LmdDesugarFailure {
    pub error: LmdError,
    /// The cell (code block or code span) in the source.
    pub location: SourceInfo,
}

/// Desugar every `{lmd}` cell in `blocks` in place, block cells and inline
/// cells alike. Returns one failure per cell that was left as it was.
pub fn desugar_lmd_cells(blocks: &mut Vec<Block>) -> Vec<LmdDesugarFailure> {
    let mut errors = Vec::new();
    // Inline cells: `` `{lmd} expr` `` is a `Code` whose text carries the
    // tag. The tag is dropped, the expression desugared, and the node
    // marked; the execute stage evaluates it as an expression.
    for_each_inline_list_mut(blocks, &mut |list| {
        for inline in list.iter_mut() {
            let Inline::Code(code) = inline else {
                continue;
            };
            let Some(expr) = inline_lmd_source(&code.attr, &code.text) else {
                continue;
            };
            match desugar_lua_expr(expr, 0) {
                Ok(lua) => {
                    code.text = lua;
                    code.attr
                        .2
                        .insert(LMD_PENDING_ATTR.to_string(), LMD_PENDING_VALUE.to_string());
                }
                Err(err) => errors.push(LmdDesugarFailure {
                    error: err,
                    location: code.source_info.clone(),
                }),
            }
        }
    });
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
                    cell.attr
                        .2
                        .insert(LMD_PENDING_ATTR.to_string(), LMD_PENDING_VALUE.to_string());
                }
                Err(err) => errors.push(LmdDesugarFailure {
                    error: err,
                    location: cell.source_info.clone(),
                }),
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
        for failure in errors {
            trace_event!(ctx, EventLevel::Warn, "{}", failure.error);
            ctx.diagnostics.push(
                DiagnosticMessageBuilder::warning("lmd cell could not be desugared")
                    .with_location(failure.location)
                    .problem(failure.error.to_string())
                    .add_hint("The cell is left as written and will not run.")
                    .build(),
            );
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
        assert_eq!(cell.attr.2.get("lmd").map(String::as_str), Some("pending"));
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
    fn inline_cell_is_desugared_and_marked() {
        let mut blocks = parse("Value `{lmd} f({=[ *x* ]=})` here and `plain` code.\n");
        let errors = desugar_lmd_cells(&mut blocks);
        assert!(errors.is_empty(), "{errors:?}");
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected Para");
        };
        let codes: Vec<&quarto_pandoc_types::inline::Code> = p
            .content
            .iter()
            .filter_map(|i| match i {
                Inline::Code(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(codes.len(), 2);
        assert!(
            codes[0].text.starts_with("f(pandoc.Inlines({"),
            "{}",
            codes[0].text
        );
        assert_eq!(
            codes[0].attr.2.get("lmd").map(String::as_str),
            Some("pending")
        );
        assert_eq!(codes[1].text, "plain");
        assert!(codes[1].attr.2.get("lmd").is_none());
    }

    #[test]
    fn a_bare_tag_mention_is_not_an_inline_cell() {
        let mut blocks = parse("The `{lmd}` cell and the `{lmd} ` tag.\n");
        let before = blocks.clone();
        let errors = desugar_lmd_cells(&mut blocks);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(blocks, before);
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
        assert!(matches!(errors[0].error, LmdError::LuaParse { .. }));
        assert_eq!(blocks, before);
    }
}
