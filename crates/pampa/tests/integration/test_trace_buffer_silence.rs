/*
 * test_trace_buffer_silence.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The qmd reader takes an `output_stream` that is a verbose trace, shown by
 * `pampa -v` and sunk by every pipeline caller. On well-formed input it must
 * stay empty: the tree-sitter dispatch catch-all in `pandoc/treesitter.rs`
 * writes a `[TOP-LEVEL MISSING NODE]` line there for any node kind without
 * an arm, and marker tokens that a parent processor consumes by name must
 * have an arm so they do not look unhandled.
 *
 * Context: GH #847 / bd-okfxemnj — pipe-table delimiter dashes and the
 * image opener reached the catch-all, and the Lua `pandoc.read` binding
 * pointed the trace at stderr, so every delimiter dash printed a line.
 * Plan: claude-notes/plans/2026-10-15-pandoc-read-delimiter-dash-noise.md
 */

use pampa::pandoc::{Alignment, Block, Inline};
use pampa::readers;

/// Parse `input` and return the document together with everything the
/// reader wrote to its verbose trace stream.
fn parse_with_trace(input: &str) -> (pampa::pandoc::Pandoc, String) {
    let mut trace = Vec::new();
    let (doc, _ctx, _warnings) =
        readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut trace, true, None)
            .expect("input is well-formed and must parse");
    (doc, String::from_utf8(trace).expect("trace is UTF-8"))
}

fn assert_trace_silent(trace: &str, what: &str) {
    assert!(
        trace.is_empty(),
        "the reader's verbose trace must be empty for {what}, got:\n{trace}"
    );
}

/// The issue's table: two default-aligned columns, three dashes each.
#[test]
fn pipe_table_delimiter_dashes_do_not_reach_the_catch_all() {
    let (doc, trace) = parse_with_trace("| a | b |\n|---|---|\n| 1 | 2 |\n");
    assert_trace_silent(&trace, "a pipe table");
    assert!(
        matches!(doc.blocks.as_slice(), [Block::Table(_)]),
        "expected one Table block, got {:?}",
        doc.blocks
    );
}

/// Every alignment form: `:--` (left), `:-:` (center), `--:` (right) and
/// `---` (default). The aligned forms add `pipe_table_align_left` /
/// `pipe_table_align_right` children next to the dashes; all of them are
/// consumed by `process_pipe_table_delimiter_cell` by node name.
#[test]
fn pipe_table_alignment_markers_do_not_reach_the_catch_all() {
    let (doc, trace) =
        parse_with_trace("| l | c | r | d |\n|:--|:-:|--:|---|\n| 1 | 2 | 3 | 4 |\n");
    assert_trace_silent(&trace, "a pipe table with all four alignments");

    let [Block::Table(table)] = doc.blocks.as_slice() else {
        panic!("expected one Table block, got {:?}", doc.blocks);
    };
    let alignments: Vec<Alignment> = table.colspec.iter().map(|(a, _)| a.clone()).collect();
    assert_eq!(
        alignments,
        vec![
            Alignment::Left,
            Alignment::Center,
            Alignment::Right,
            Alignment::Default
        ],
        "the explicit marker arms must not change how alignments are read"
    );
}

/// The `![` opener of an inline image is an anonymous token consumed by
/// `process_pandoc_image`; it must not look unhandled either. The image
/// sits in running text so the paragraph stays a Paragraph (an image alone
/// on a line becomes an implicit Figure).
#[test]
fn image_opener_does_not_reach_the_catch_all() {
    let (doc, trace) = parse_with_trace("See ![alt text](a.png) here.\n");
    assert_trace_silent(&trace, "an inline image");

    let [Block::Paragraph(para)] = doc.blocks.as_slice() else {
        panic!("expected one Paragraph block, got {:?}", doc.blocks);
    };
    assert!(
        para.content.iter().any(|i| matches!(i, Inline::Image(_))),
        "expected an Image inline in the paragraph, got {:?}",
        para.content
    );
}
