/*
 * pandoc/desugar.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The structural desugaring set.
//!
//! The qmd reader's postprocess pass does two kinds of work. Most of it is
//! concrete-syntax clean-up: fixes to what tree-sitter produced from *this
//! spelling* of the markdown (trailing line breaks, header attributes, the
//! `* []` empty item, captions parsed as table rows, and so on). Those only
//! make sense on reader output, and the shapes they act on cannot be built
//! from Lua.
//!
//! The rest rewrites AST *shapes* that are legal pandoc AST on their own
//! and that an author can produce either by writing markdown or by
//! constructing nodes:
//!
//! 1. `Div(.list-table)` → `Table` (a malformed one warns Q-2-35 and is
//!    left as is);
//! 2. `Div(.definition-list)` → `DefinitionList` (a malformed one is left
//!    as is, silently, as the reader does);
//! 3. `Para([Image])` with non-empty alt text → `Figure` captioned by the
//!    alt text (pandoc's `implicit_figures`), at any depth, as pandoc and
//!    the reader both do.
//!
//! That set is the **structural desugaring contract**: every block list
//! that enters the render pipeline *other than through the qmd reader* is
//! rewritten by it, in that order, exactly once. It is owed to lmd cell
//! results, lmd fragments (by construction: they go through the reader),
//! `pandoc.read(…, "json")`, pampa's `--from json`, and document import.
//! It is *not* owed to what user Lua filters, shortcode handlers and
//! components return: those are filter semantics, where pandoc and
//! Quarto 1 both leave constructed nodes alone. See
//! `dev-docs/syntax-notes.md` § Structural desugaring contract, and
//! `claude-notes/plans/2026-10-15-lmd-ast-desugar-contract.md`
//! (bd-7gqr148v).
//!
//! The pass is idempotent on its own output except for an *invalid*
//! list-table div, which warns again, so callers run it over what a
//! source produced, never over a whole document that the reader already
//! processed.
//!
//! The reader shares the per-node rewrites ([`desugar_div`],
//! [`implicit_figure`]) with this module so the two cannot drift; what it
//! does not share is the traversal, because it fuses them with the
//! concrete-syntax work in one pass.

use std::cell::RefCell;

use crate::filter_context::FilterContext;
use crate::filters::{
    Filter, FilterReturn, FilterReturn::FilterResult, FilterReturn::Unchanged,
    topdown_traverse_blocks, topdown_traverse_inlines,
};
use crate::pandoc::treesitter_utils::postprocess::{
    ListTableValidation, is_valid_definition_list_div, transform_definition_list_div,
    transform_list_table_div, validate_list_table_div,
};
use crate::pandoc::{
    Attr, Block, Blocks, Caption, Div, Figure, Inline, Inlines, Pandoc, Paragraph, Plain,
};
use crate::utils::diagnostic_collector::DiagnosticCollector;
use hashlink::LinkedHashMap;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::AttrSourceInfo;

/// Items 1 and 2 of the set, for one div: a valid list-table becomes a
/// `Table`, a valid definition-list a `DefinitionList`, an invalid
/// list-table warns Q-2-35 into `collector` and stays a div, and any other
/// div is unchanged. Neither rewrite recurses into its result, matching
/// the reader.
pub(crate) fn desugar_div(
    div: Div,
    collector: &mut DiagnosticCollector,
) -> FilterReturn<Div, Blocks> {
    match validate_list_table_div(&div) {
        ListTableValidation::Valid => FilterResult(vec![transform_list_table_div(div)], false),
        ListTableValidation::Invalid { reason, location } => {
            collector.add(
                DiagnosticMessageBuilder::warning("Invalid List-Table Structure")
                    .with_code("Q-2-35")
                    .with_location(location)
                    .problem(reason)
                    .add_hint("Check the list-table documentation for the correct structure?")
                    .build(),
            );
            Unchanged(div)
        }
        ListTableValidation::NotListTable => {
            if is_valid_definition_list_div(&div) {
                FilterResult(vec![transform_definition_list_div(div)], false)
            } else {
                Unchanged(div)
            }
        }
    }
}

/// Item 3 of the set, for one paragraph: `Some(figure)` when the paragraph
/// is a single image with non-empty alt text. The image's id moves to the
/// figure; its classes and attributes stay on the image; the alt text
/// becomes the figure's caption and stays the image's alt.
pub(crate) fn implicit_figure(para: &Paragraph) -> Option<Figure> {
    let [Inline::Image(image)] = para.content.as_slice() else {
        return None;
    };
    if image.content.is_empty() {
        return None;
    }

    let figure_attr: Attr = (image.attr.0.clone(), vec![], LinkedHashMap::new());
    let image_attr: Attr = (String::new(), image.attr.1.clone(), image.attr.2.clone());

    // Split attr_source between figure and image the same way.
    let figure_attr_source = AttrSourceInfo {
        id: image.attr_source.id.clone(),
        classes: vec![],
        attributes: vec![],
    };
    let image_attr_source = AttrSourceInfo {
        id: None,
        classes: image.attr_source.classes.clone(),
        attributes: image.attr_source.attributes.clone(),
    };

    let mut new_image = image.clone();
    new_image.attr = image_attr;
    new_image.attr_source = image_attr_source;

    Some(Figure {
        attr: figure_attr,
        caption: Caption {
            short: None,
            long: Some(vec![Block::Plain(Plain {
                content: image.content.clone(),
                // Caption text comes from the image's alt text.
                source_info: image.source_info.clone(),
            })]),
            source_info: image.source_info.clone(),
        },
        content: vec![Block::Plain(Plain {
            content: vec![Inline::Image(new_image)],
            source_info: image.source_info.clone(),
        })],
        // The figure spans the whole paragraph.
        source_info: para.source_info.clone(),
        attr_source: figure_attr_source,
    })
}

/// The set as a filter over `collector`.
fn desugar_filter<'a>(collector: &'a RefCell<&'a mut DiagnosticCollector>) -> Filter<'a> {
    Filter::new()
        .with_div(move |div, _ctx| desugar_div(div, &mut collector.borrow_mut()))
        .with_paragraph(|para, _ctx| match implicit_figure(&para) {
            Some(figure) => FilterResult(vec![Block::Figure(figure)], true),
            None => Unchanged(para),
        })
}

/// Apply the set to a block list.
pub fn desugar_blocks(blocks: Blocks, collector: &mut DiagnosticCollector) -> Blocks {
    let collector = RefCell::new(collector);
    let mut filter = desugar_filter(&collector);
    let mut ctx = FilterContext::new();
    topdown_traverse_blocks(blocks, &mut filter, &mut ctx)
}

/// Apply the set to an inline list. Inlines carry no sugar of their own;
/// this descends into the blocks inside them (a `Note`'s content).
pub fn desugar_inlines(inlines: Inlines, collector: &mut DiagnosticCollector) -> Inlines {
    let collector = RefCell::new(collector);
    let mut filter = desugar_filter(&collector);
    let mut ctx = FilterContext::new();
    topdown_traverse_inlines(inlines, &mut filter, &mut ctx)
}

/// Apply the set to a whole document's blocks (metadata is untouched).
pub fn desugar_document(mut doc: Pandoc, collector: &mut DiagnosticCollector) -> Pandoc {
    doc.blocks = desugar_blocks(doc.blocks, collector);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::location::empty_source_info;
    use crate::pandoc::{BulletList, Image, Note, Str};
    use quarto_pandoc_types::TargetSourceInfo;

    fn plain(text: &str) -> Block {
        Block::Plain(Plain {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: empty_source_info(),
            })],
            source_info: empty_source_info(),
        })
    }

    fn bullets(items: Vec<Blocks>) -> Block {
        Block::BulletList(BulletList {
            content: items,
            source_info: empty_source_info(),
        })
    }

    fn div(class: &str, content: Blocks) -> Block {
        Block::Div(Div {
            attr: (String::new(), vec![class.to_string()], LinkedHashMap::new()),
            content,
            source_info: empty_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn list_table() -> Block {
        div(
            "list-table",
            vec![bullets(vec![
                vec![bullets(vec![vec![plain("k")], vec![plain("v")]])],
                vec![bullets(vec![vec![plain("a")], vec![plain("1")]])],
            ])],
        )
    }

    fn image_para(alt: &str, id: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Image(Image {
                attr: (
                    id.to_string(),
                    vec!["cls".to_string()],
                    LinkedHashMap::new(),
                ),
                content: if alt.is_empty() {
                    vec![]
                } else {
                    vec![Inline::Str(Str {
                        text: alt.to_string(),
                        source_info: empty_source_info(),
                    })]
                },
                target: ("img.png".to_string(), String::new()),
                source_info: empty_source_info(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty(),
            })],
            source_info: empty_source_info(),
        })
    }

    #[test]
    fn a_list_table_div_becomes_a_table() {
        let mut collector = DiagnosticCollector::new();
        let out = desugar_blocks(vec![list_table()], &mut collector);
        assert!(matches!(out.as_slice(), [Block::Table(_)]), "{out:?}");
        assert!(collector.diagnostics().is_empty());
    }

    #[test]
    fn a_definition_list_div_becomes_a_definition_list() {
        let mut collector = DiagnosticCollector::new();
        let input = div(
            "definition-list",
            vec![bullets(vec![vec![
                plain("term"),
                bullets(vec![vec![plain("def")]]),
            ]])],
        );
        let out = desugar_blocks(vec![input], &mut collector);
        assert!(
            matches!(out.as_slice(), [Block::DefinitionList(_)]),
            "{out:?}"
        );
    }

    #[test]
    fn an_invalid_list_table_div_warns_and_stays() {
        let mut collector = DiagnosticCollector::new();
        let out = desugar_blocks(vec![div("list-table", vec![plain("x")])], &mut collector);
        assert!(matches!(out.as_slice(), [Block::Div(_)]), "{out:?}");
        let diags = collector.diagnostics();
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-2-35"));
    }

    #[test]
    fn a_single_image_paragraph_with_alt_becomes_a_figure_keeping_the_id() {
        let mut collector = DiagnosticCollector::new();
        let out = desugar_blocks(vec![image_para("alt", "fig-x")], &mut collector);
        let [Block::Figure(figure)] = out.as_slice() else {
            panic!("expected Figure, got {out:?}");
        };
        assert_eq!(figure.attr.0, "fig-x");
        let [Block::Plain(body)] = figure.content.as_slice() else {
            panic!("{:?}", figure.content);
        };
        let [Inline::Image(image)] = body.content.as_slice() else {
            panic!("{:?}", body.content);
        };
        assert_eq!(image.attr.0, "");
        assert_eq!(image.attr.1, vec!["cls".to_string()]);
        assert!(figure.caption.long.is_some());
    }

    #[test]
    fn an_image_paragraph_without_alt_stays_a_paragraph() {
        let mut collector = DiagnosticCollector::new();
        let out = desugar_blocks(vec![image_para("", "")], &mut collector);
        assert!(matches!(out.as_slice(), [Block::Paragraph(_)]), "{out:?}");
    }

    #[test]
    fn the_set_applies_at_any_depth() {
        let mut collector = DiagnosticCollector::new();
        let input = Block::BlockQuote(crate::pandoc::BlockQuote {
            content: vec![div("wrapper", vec![list_table(), image_para("alt", "")])],
            source_info: empty_source_info(),
        });
        let out = desugar_blocks(vec![input], &mut collector);
        let [Block::BlockQuote(quote)] = out.as_slice() else {
            panic!("{out:?}");
        };
        let [Block::Div(wrapper)] = quote.content.as_slice() else {
            panic!("{:?}", quote.content);
        };
        assert!(
            matches!(
                wrapper.content.as_slice(),
                [Block::Table(_), Block::Figure(_)]
            ),
            "{:?}",
            wrapper.content
        );
    }

    #[test]
    fn inlines_are_desugared_through_a_note() {
        let mut collector = DiagnosticCollector::new();
        let note = Inline::Note(Note {
            content: vec![list_table()],
            source_info: empty_source_info(),
        });
        let out = desugar_inlines(vec![note], &mut collector);
        let [Inline::Note(note)] = out.as_slice() else {
            panic!("{out:?}");
        };
        assert!(
            matches!(note.content.as_slice(), [Block::Table(_)]),
            "{:?}",
            note.content
        );
    }

    #[test]
    fn the_pass_is_idempotent_on_valid_output() {
        let mut collector = DiagnosticCollector::new();
        let once = desugar_blocks(vec![list_table(), image_para("alt", "")], &mut collector);
        let twice = desugar_blocks(once.clone(), &mut collector);
        assert_eq!(once, twice);
        assert!(collector.diagnostics().is_empty());
    }
}
