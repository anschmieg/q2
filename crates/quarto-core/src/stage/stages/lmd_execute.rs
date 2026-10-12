/*
 * stage/stages/lmd_execute.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * lmd exploration spike 2: run desugared `{lmd}` cells.
 */

//! `LmdExecuteStage`: run every cell `LmdDesugarStage` marked, in document
//! order, in one Lua state per document, and replace each cell with the
//! blocks it returned.
//!
//! Sits after engine execution and right before `user-filters-pre`, so a
//! cell sees engine output and lives where the eventual "a cell is a
//! filter" story lives. The contract for what a cell may return is in
//! `pampa::lua::lmd`: a `Block`, a list of `Block`s, or `nil` (the cell
//! vanishes); `Inlines` and strings are errors.
//!
//! Cell options use Lua comment syntax, `--| key: value`, at the top of the
//! cell. `echo: true` keeps the desugared Lua visible as a plain `lmd` code
//! block above the result (off by default). A cell that fails, at compile
//! time, at run time, or by returning the wrong kind of value, is reported
//! as an error diagnostic and left in the document as that same displayed
//! block, so the rest of the page still renders and the failing Lua can be
//! read.
//!
//! Spike 2 of `claude-notes/plans/2026-10-12-lmd-exploration.md`.

use async_trait::async_trait;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, CodeBlock};
use quarto_source_map::SourceInfo;
use std::collections::HashMap;

use crate::ast_walk::for_each_block_list_mut;
use crate::cell_options::{options_to_config, partition_cell_options};
use crate::lmd::{LMD_LANG, LMD_PENDING_ATTR, LMD_PENDING_VALUE};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use pampa::lua::lmd::{LmdCellError, LmdCellValue, LmdRuntime};

#[derive(Default)]
pub struct LmdExecuteStage;

impl LmdExecuteStage {
    pub fn new() -> Self {
        Self
    }
}

/// A marked cell found by the pre-order walk, with the attribute value the
/// splice pass will look for.
struct PendingCell {
    id: usize,
    text: String,
    source_info: SourceInfo,
}

fn is_pending_cell(block: &Block) -> bool {
    matches!(block, Block::CodeBlock(cb)
        if cb.attr.2.get(LMD_PENDING_ATTR).map(String::as_str) == Some(LMD_PENDING_VALUE))
}

/// Collect marked cells in document order (pre-order), overwriting each
/// one's marker with its sequence number so the splice pass can find it
/// after the async run loop. Extend the arms if `Block` gains a container.
fn collect_pending_cells(blocks: &mut [Block], out: &mut Vec<PendingCell>) {
    for block in blocks.iter_mut() {
        if is_pending_cell(block) {
            let Block::CodeBlock(cb) = block else {
                unreachable!("is_pending_cell matched a CodeBlock");
            };
            let id = out.len();
            cb.attr
                .2
                .insert(LMD_PENDING_ATTR.to_string(), id.to_string());
            out.push(PendingCell {
                id,
                text: cb.text.clone(),
                source_info: cb.source_info.clone(),
            });
            continue;
        }
        match block {
            Block::BlockQuote(bq) => collect_pending_cells(&mut bq.content, out),
            Block::Div(div) => collect_pending_cells(&mut div.content, out),
            Block::Figure(fig) => collect_pending_cells(&mut fig.content, out),
            Block::OrderedList(list) => {
                for item in &mut list.content {
                    collect_pending_cells(item, out);
                }
            }
            Block::BulletList(list) => {
                for item in &mut list.content {
                    collect_pending_cells(item, out);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in &mut dl.content {
                    for def in defs {
                        collect_pending_cells(def, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The displayed form of a cell: a plain `lmd` code block holding its
/// (option-stripped) desugared Lua. Used for `echo: true` and for failures.
fn displayed_cell(code: &str, source_info: &SourceInfo) -> Block {
    Block::CodeBlock(CodeBlock {
        attr: (
            String::new(),
            vec![LMD_LANG.to_string()],
            Default::default(),
        ),
        text: code.to_string(),
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn cell_error(
    title: &str,
    cell: &PendingCell,
    problem: String,
    hint: Option<&str>,
) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error(title)
        .with_location(cell.source_info.clone())
        .problem(problem);
    if let Some(hint) = hint {
        builder = builder.add_hint(hint);
    }
    builder.build()
}

/// Run every marked cell in `blocks` and splice the results in place.
/// Diagnostics (cell failures, `quarto.warn` calls) go to `diagnostics`.
pub async fn execute_lmd_cells(
    blocks: &mut Vec<Block>,
    runtime: &LmdRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let mut cells = Vec::new();
    collect_pending_cells(blocks, &mut cells);
    if cells.is_empty() {
        return;
    }

    let mut results: HashMap<usize, Vec<Block>> = HashMap::new();
    for cell in &cells {
        // Cell options use Lua's comment syntax; the language key is
        // `lua` because `lmd` is unknown to the comment table and would
        // default to `#`.
        let (code, echo) = match partition_cell_options("lua", &cell.text, cell.source_info.clone())
        {
            Ok(partitioned) => {
                let echo = partitioned.options.map(options_to_config).is_some_and(
                    |(config, option_diags)| {
                        diagnostics.extend(option_diags);
                        config
                            .get("echo")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                    },
                );
                (partitioned.code, echo)
            }
            Err(err) => {
                diagnostics.push(cell_error(
                    "lmd cell options are not valid",
                    cell,
                    err.to_string(),
                    None,
                ));
                (cell.text.clone(), false)
            }
        };

        let chunk_name = format!("lmd cell {}", cell.id + 1);
        let mut output: Vec<Block> = Vec::new();
        match runtime.run_cell(&code, &chunk_name).await {
            Ok(LmdCellValue::Nothing) => {
                if echo {
                    output.push(displayed_cell(&code, &cell.source_info));
                }
            }
            Ok(LmdCellValue::Blocks(result)) => {
                if echo {
                    output.push(displayed_cell(&code, &cell.source_info));
                }
                output.extend(result);
            }
            Err(err) => {
                let (title, hint) = match &err {
                    LmdCellError::Lua(_) => ("lmd cell failed", None),
                    LmdCellError::NotBlocks { .. } => (
                        "lmd cell did not return blocks",
                        Some(
                            "Return a Block, a list of Blocks, or nil. Write `{=: … :=}` for a markdown fragment that is blocks.",
                        ),
                    ),
                };
                diagnostics.push(cell_error(title, cell, err.to_string(), hint));
                output.push(displayed_cell(&code, &cell.source_info));
            }
        }
        results.insert(cell.id, output);
    }
    diagnostics.extend(runtime.take_diagnostics());

    for_each_block_list_mut(blocks, &mut |list| {
        if !list.iter().any(is_numbered_cell) {
            return;
        }
        let old = std::mem::take(list);
        for block in old {
            match numbered_cell_id(&block) {
                Some(id) => list.extend(results.remove(&id).unwrap_or_default()),
                None => list.push(block),
            }
        }
    });
}

fn numbered_cell_id(block: &Block) -> Option<usize> {
    let Block::CodeBlock(cb) = block else {
        return None;
    };
    cb.attr.2.get(LMD_PENDING_ATTR)?.parse().ok()
}

fn is_numbered_cell(block: &Block) -> bool {
    numbered_cell_id(block).is_some()
}

#[async_trait(?Send)]
impl PipelineStage for LmdExecuteStage {
    fn name(&self) -> &str {
        "lmd-execute"
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

        // Cheap check before building a Lua state: most documents have no
        // lmd cells.
        let mut any = false;
        for_each_block_list_mut(&mut doc.ast.blocks, &mut |list| {
            any |= list.iter().any(is_pending_cell);
        });
        if !any {
            return Ok(PipelineData::DocumentAst(doc));
        }

        // Same environment a Lua filter gets (see `UserFiltersStage`),
        // with the document standing in for the script path.
        let attribution: Option<std::sync::Arc<dyn pampa::attribution::AttributionLookup>> =
            ctx.attribution_data.as_ref().map(|data| {
                std::sync::Arc::new(crate::attribution::AttributionLookupHandle::new(
                    data.clone(),
                )) as std::sync::Arc<dyn pampa::attribution::AttributionLookup>
            });
        let runtime = LmdRuntime::new(
            ctx.runtime.clone(),
            ctx.format.lua_format(),
            &ctx.document.input,
            attribution,
        )
        .map_err(|err| PipelineError::stage_error(self.name(), err.to_string()))?;

        let mut diagnostics = Vec::new();
        execute_lmd_cells(&mut doc.ast.blocks, &runtime, &mut diagnostics).await;
        for diag in &diagnostics {
            trace_event!(ctx, EventLevel::Warn, "{}", diag.title);
        }
        ctx.diagnostics.extend(diagnostics);

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lmd::desugar_lua;
    use quarto_system_runtime::NativeRuntime;
    use std::path::Path;
    use std::sync::Arc;

    fn runtime() -> LmdRuntime {
        LmdRuntime::new(
            Arc::new(NativeRuntime::new()),
            "html",
            Path::new("doc.qmd"),
            None,
        )
        .expect("lmd runtime")
    }

    /// Parse `qmd`, desugar its `{lmd}` cells (what the desugar stage
    /// does), and run them.
    fn render(qmd: &str) -> (Vec<Block>, Vec<DiagnosticMessage>) {
        let mut sink: Vec<u8> = Vec::new();
        let (ast, _ctx, _warnings) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .expect("test document parses");
        let mut blocks = ast.blocks;
        let errors = super::super::lmd_desugar::desugar_lmd_cells(&mut blocks);
        assert!(errors.is_empty(), "{errors:?}");
        let mut diagnostics = Vec::new();
        pollster::block_on(execute_lmd_cells(&mut blocks, &runtime(), &mut diagnostics));
        (blocks, diagnostics)
    }

    fn para_text(block: &Block) -> String {
        let Block::Paragraph(p) = block else {
            panic!("expected Para, got {block:?}");
        };
        p.content
            .iter()
            .map(|i| match i {
                quarto_pandoc_types::inline::Inline::Str(s) => s.text.clone(),
                quarto_pandoc_types::inline::Inline::Space(_) => " ".to_string(),
                quarto_pandoc_types::inline::Inline::Strong(s) => format!(
                    "**{}**",
                    s.content
                        .iter()
                        .map(|i| match i {
                            quarto_pandoc_types::inline::Inline::Str(s) => s.text.clone(),
                            _ => "?".to_string(),
                        })
                        .collect::<String>()
                ),
                _ => "?".to_string(),
            })
            .collect()
    }

    #[test]
    fn a_cell_is_replaced_by_the_blocks_it_returns() {
        let (blocks, diags) =
            render("Before\n\n```{lmd}\nreturn {=: **hello** :=}\n```\n\nAfter\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert_eq!(para_text(&blocks[0]), "Before");
        assert_eq!(para_text(&blocks[1]), "**hello**");
        assert_eq!(para_text(&blocks[2]), "After");
        assert!(!blocks.iter().any(is_pending_cell));
    }

    #[test]
    fn the_motivating_example_with_immediate_values() {
        let qmd = "::: foo\n\n```{lmd}\nlocal out = pandoc.Blocks({})\nfor _, v in ipairs({'a', 'b'}) do\n  out:extend({=: **`{lmd} v`** :=})\nend\nreturn out\n```\n\n:::\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div");
        };
        assert_eq!(div.content.len(), 2, "{:?}", div.content);
        assert_eq!(para_text(&div.content[0]), "**a**");
        assert_eq!(para_text(&div.content[1]), "**b**");
    }

    #[test]
    fn nil_removes_the_cell_and_state_is_shared_in_document_order() {
        let qmd =
            "```{lmd}\nshared = 'seen'\n```\n\n```{lmd}\nreturn {=: **`{lmd} shared`** :=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert_eq!(para_text(&blocks[0]), "**seen**");
    }

    #[test]
    fn echo_keeps_the_desugared_lua_above_the_result() {
        let qmd = "```{lmd}\n--| echo: true\nreturn {=: hi :=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        let Block::CodeBlock(shown) = &blocks[0] else {
            panic!("expected the echoed cell first");
        };
        assert_eq!(shown.attr.1, vec!["lmd".to_string()]);
        assert!(
            shown.text.starts_with("return pandoc.Blocks({"),
            "{}",
            shown.text
        );
        assert!(!shown.text.contains("echo"), "{}", shown.text);
        assert_eq!(para_text(&blocks[1]), "hi");
    }

    #[test]
    fn a_failing_cell_is_reported_and_left_displayed() {
        let qmd = "```{lmd}\nerror('boom')\n```\n\n```{lmd}\nreturn {=[ not blocks ]=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags[0].title.contains("lmd cell failed"), "{:?}", diags[0]);
        assert!(
            diags[1].title.contains("did not return blocks"),
            "{:?}",
            diags[1]
        );
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        for block in &blocks {
            let Block::CodeBlock(cb) = block else {
                panic!("expected a displayed cell, got {block:?}");
            };
            assert_eq!(cb.attr.1, vec!["lmd".to_string()]);
            assert!(cb.attr.2.get(LMD_PENDING_ATTR).is_none());
        }
    }

    #[test]
    fn cells_nested_in_containers_run_in_document_order() {
        let qmd = "```{lmd}\nn = 1\n```\n\n::: box\n\n```{lmd}\nn = n + 1\nreturn {=: **`{lmd} tostring(n)`** :=}\n```\n\n:::\n\n```{lmd}\nreturn {=: **`{lmd} tostring(n * 10)`** :=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div");
        };
        assert_eq!(para_text(&div.content[0]), "**2**");
        assert_eq!(para_text(&blocks[1]), "**20**");
    }

    #[test]
    fn documents_without_cells_are_untouched() {
        let qmd = "Plain\n\n```lua\nreturn 1\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty());
        assert_eq!(blocks.len(), 2);
        let _ = desugar_lua; // keep the import honest for future tests
    }
}
