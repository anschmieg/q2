/*
 * lmd/desugar.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! lmd source → plain Lua: every `{= markdown =}` becomes `pandoc.*` calls.
//!
//! The Lua is parsed with `tree-sitter-lmd`; each `markdown_fragment` node
//! (never nested in the tree: fragment content is one raw token) is cut out
//! and replaced by the emitted expression. Markdown inside a fragment is
//! parsed with pampa's qmd reader; lmd code inside *that* markdown comes
//! back through [`desugar_lua_at`] from the emitter, which is how fragments
//! nest.

use quarto_pandoc_types::block::Block;

use super::emit::Emitter;

/// Why an lmd cell could not be desugared. Every variant leaves the cell as
/// it was; the stage reports the error as a warning.
#[derive(Debug, thiserror::Error)]
pub enum LmdError {
    #[error("lmd: Lua syntax error at line {line}, column {column}")]
    LuaParse { line: usize, column: usize },
    #[error("lmd: markdown fragment failed to parse: {0}")]
    MarkdownParse(String),
    #[error("lmd: `{0}` nodes are not supported inside markdown fragments yet")]
    Unsupported(&'static str),
    #[error("lmd: `{{=[ … ]=}}` asks for inlines but the markdown parsed to {0} blocks")]
    NotInlines(usize),
}

/// Which of the three fragment forms a `markdown_fragment` node is, read
/// off the node kind of its `start` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentKind {
    /// `{= … =}`: `pandoc.Nodes`, the surrounding context decides.
    Nodes,
    /// `{=: … :=}`: `pandoc.Blocks`.
    Blocks,
    /// `{=[ … ]=}`: `pandoc.Inlines`.
    Inlines,
}

impl FragmentKind {
    fn of(fragment: tree_sitter::Node<'_>) -> Self {
        let start = fragment
            .child_by_field_name("start")
            .expect("markdown_fragment has a start field");
        match start.kind() {
            tree_sitter_lmd::BLOCKS_START => Self::Blocks,
            tree_sitter_lmd::INLINES_START => Self::Inlines,
            tree_sitter_lmd::NODES_START => Self::Nodes,
            other => unreachable!("unknown fragment start kind {other:?}"),
        }
    }
}

/// Desugar `src` (lmd) into plain Lua.
pub fn desugar_lua(src: &str) -> Result<String, LmdError> {
    desugar_lua_at(src, 0)
}

/// [`desugar_lua`] for a chunk that will be spliced in at column `base`:
/// continuation lines of each emitted fragment are indented past it.
pub(super) fn desugar_lua_at(src: &str, base: usize) -> Result<String, LmdError> {
    desugar_inner(src, base, 0)
}

/// [`desugar_lua_at`] for a single Lua *expression* (what an inline
/// `` `{lmd} expr` `` holds). A bare expression is not a chunk, so it is
/// parsed as `return <expr>` and the prefix is stripped again.
pub fn desugar_lua_expr(src: &str, base: usize) -> Result<String, LmdError> {
    const PREFIX: &str = "return ";
    let wrapped = format!("{PREFIX}{src}");
    let out = desugar_inner(&wrapped, base, PREFIX.len())?;
    Ok(out[PREFIX.len()..].to_string())
}

/// `first_line_offset` is subtracted from the column of fragments on the
/// first line, for callers that prepended text to `src`.
fn desugar_inner(src: &str, base: usize, first_line_offset: usize) -> Result<String, LmdError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_lmd::LANGUAGE.into())
        .expect("lmd grammar loads");
    let tree = parser
        .parse(src, None)
        .expect("tree-sitter parse returns a tree");
    let root = tree.root_node();
    if root.has_error() {
        let err = first_error(root);
        let pos = err.start_position();
        return Err(LmdError::LuaParse {
            line: pos.row + 1,
            column: pos.column + 1,
        });
    }

    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    for fragment in fragments(root) {
        out.push_str(&src[last..fragment.start_byte()]);
        let content = fragment
            .child_by_field_name("content")
            .map_or("", |c| &src[c.byte_range()]);
        let pos = fragment.start_position();
        let shift = if pos.row == 0 { first_line_offset } else { 0 };
        let column = base + pos.column.saturating_sub(shift);
        out.push_str(&emit_fragment(content, column, FragmentKind::of(fragment))?);
        last = fragment.end_byte();
    }
    out.push_str(&src[last..]);
    Ok(out)
}

/// Parse one fragment's markdown and print it as a Lua expression.
///
/// - `{= … =}` becomes `pandoc.Nodes({ blocks })`: the reader's blocks,
///   handed to a constructor that is meant to adapt them to the context
///   (not implemented yet; see `pandoc.Nodes` in pampa).
/// - `{=: … :=}` becomes `pandoc.Blocks({ blocks })`.
/// - `{=[ … ]=}` becomes `pandoc.Inlines({ inlines })`, where the markdown
///   must parse to a single paragraph (or nothing); anything else is an
///   error, since the author asked for inlines.
fn emit_fragment(markdown: &str, column: usize, kind: FragmentKind) -> Result<String, LmdError> {
    let markdown = markdown.trim();
    let blocks = parse_markdown(markdown)?;
    let mut emitter = Emitter::new(column);
    match kind {
        FragmentKind::Nodes => emitter.blocks_call("Nodes", &blocks)?,
        FragmentKind::Blocks => emitter.blocks_call("Blocks", &blocks)?,
        FragmentKind::Inlines => match blocks.as_slice() {
            [] => emitter.inlines_call("Inlines", &[])?,
            [Block::Paragraph(p)] => emitter.inlines_call("Inlines", &p.content)?,
            [Block::Plain(p)] => emitter.inlines_call("Inlines", &p.content)?,
            other => return Err(LmdError::NotInlines(other.len())),
        },
    }
    Ok(emitter.finish())
}

fn parse_markdown(markdown: &str) -> Result<Vec<Block>, LmdError> {
    if markdown.is_empty() {
        return Ok(vec![]);
    }
    let mut sink: Vec<u8> = Vec::new();
    match pampa::readers::qmd::read(
        markdown.as_bytes(),
        false,
        "fragment.lmd",
        &mut sink,
        true,
        None,
    ) {
        Ok((pandoc, _ctx, _warnings)) => Ok(pandoc.blocks),
        Err(diags) => Err(LmdError::MarkdownParse(
            diags
                .iter()
                .map(|d| d.title.clone())
                .collect::<Vec<_>>()
                .join("; "),
        )),
    }
}

/// Every `markdown_fragment` node under `root`, in source order. The walk
/// does not descend into a fragment: its content is a single raw token.
fn fragments(root: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let mut found = Vec::new();
    let mut cursor = root.walk();
    let mut descend = true;
    loop {
        let node = cursor.node();
        if node.kind() == tree_sitter_lmd::MARKDOWN_FRAGMENT {
            found.push(node);
            descend = false;
        }
        if descend && cursor.goto_first_child() {
            continue;
        }
        descend = true;
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return found;
            }
        }
    }
}

fn first_error(root: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            return node;
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return root;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_without_fragments_is_unchanged() {
        let src = "local t = { 1, x = 2 }\nreturn t";
        assert_eq!(desugar_lua(src).unwrap(), src);
    }

    #[test]
    fn unmarked_fragment_becomes_nodes() {
        let out = desugar_lua("return {= **hello** =}").unwrap();
        assert_eq!(
            out,
            r#"return pandoc.Nodes({
         pandoc.Para({
           pandoc.Strong({
             pandoc.Str("hello"),
           }),
         }),
       })"#
        );
    }

    #[test]
    fn blocks_fragment_becomes_blocks() {
        let out = desugar_lua("return {=: **hello** :=}").unwrap();
        assert!(out.starts_with("return pandoc.Blocks({\n"), "{out}");
        assert!(out.contains("pandoc.Para({"), "{out}");
    }

    #[test]
    fn inlines_fragment_becomes_inlines_without_a_para() {
        let out = desugar_lua("return {=[ **hello** ]=}").unwrap();
        assert_eq!(
            out,
            r#"return pandoc.Inlines({
         pandoc.Strong({
           pandoc.Str("hello"),
         }),
       })"#
        );
    }

    #[test]
    fn inlines_fragment_rejects_several_blocks() {
        let err = desugar_lua("return {=[ a\n\nb ]=}").unwrap_err();
        assert!(matches!(err, LmdError::NotInlines(2)), "{err}");
    }

    #[test]
    fn inlines_fragment_can_sit_inside_a_constructor() {
        let out = desugar_lua("return pandoc.Strong({=[ *a* ]=})").unwrap();
        assert!(
            out.starts_with("return pandoc.Strong(pandoc.Inlines({\n"),
            "{out}"
        );
        assert!(out.contains("pandoc.Emph({"), "{out}");
    }

    #[test]
    fn inline_lmd_code_splices_lua_back_in() {
        let src = "return tmap(xs, function(v) return {=: **`{lmd} v`** :=} end)";
        let out = desugar_lua(src).unwrap();
        assert!(out.contains("pandoc.Strong({\n"), "{out}");
        assert!(out.contains("  v,\n"), "{out}");
        assert!(out.ends_with(" end)"), "{out}");
        assert!(!out.contains("{="), "{out}");
    }

    #[test]
    fn nested_fragment_inside_inline_lmd_code() {
        let src = "return {=: *a* `{lmd} f({=[ b ]=})` :=}";
        let out = desugar_lua(src).unwrap();
        assert!(out.contains("pandoc.Emph("), "{out}");
        assert!(out.contains("f(pandoc.Inlines({"), "{out}");
        assert!(out.contains("pandoc.Str(\"b\")"), "{out}");
        assert!(!out.contains("{="), "{out}");
    }

    #[test]
    fn block_lmd_cell_inside_fragment_becomes_an_iife() {
        let src = "return {=:\nIntro\n\n```{lmd}\nreturn x\n```\n:=}";
        let out = desugar_lua(src).unwrap();
        assert!(out.starts_with("return pandoc.Blocks({\n"), "{out}");
        assert!(out.contains("(function()\n"), "{out}");
        assert!(out.contains("return x\n"), "{out}");
        assert!(out.contains("end)(),\n"), "{out}");
    }

    #[test]
    fn empty_fragments() {
        assert_eq!(
            desugar_lua("return {==}").unwrap(),
            "return pandoc.Nodes({})"
        );
        assert_eq!(
            desugar_lua("return {=::=}").unwrap(),
            "return pandoc.Blocks({})"
        );
        assert_eq!(
            desugar_lua("return {=[]=}").unwrap(),
            "return pandoc.Inlines({})"
        );
    }

    #[test]
    fn several_fragments_in_one_chunk() {
        let out = desugar_lua("local a = {= x =}\nlocal b = {=[ y ]=}").unwrap();
        assert_eq!(out.matches("pandoc.Nodes(").count(), 1);
        assert_eq!(out.matches("pandoc.Inlines(").count(), 1);
        assert!(out.contains("pandoc.Str(\"x\")"));
        assert!(out.contains("pandoc.Str(\"y\")"));
    }

    #[test]
    fn lua_syntax_error_is_reported_with_position() {
        let err = desugar_lua("return {= a =} +").unwrap_err();
        assert!(matches!(err, LmdError::LuaParse { .. }), "{err}");
    }

    #[test]
    fn unsupported_node_is_an_error() {
        let src = "return {=: | a | b |\n|---|---|\n| 1 | 2 | :=}";
        let err = desugar_lua(src).unwrap_err();
        assert!(matches!(err, LmdError::Unsupported("Table")), "{err}");
    }
}
