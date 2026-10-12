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
    #[error("lmd: `{0}` nodes are not supported inside `{{= … =}}` yet")]
    Unsupported(&'static str),
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
pub(super) fn desugar_lua_expr(src: &str, base: usize) -> Result<String, LmdError> {
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
        out.push_str(&emit_fragment(content, column)?);
        last = fragment.end_byte();
    }
    out.push_str(&src[last..]);
    Ok(out)
}

/// Parse one fragment's markdown and print it as a Lua expression.
///
/// One paragraph becomes `pandoc.Para(...)`; any other single block becomes
/// that block's constructor; several blocks become `pandoc.Blocks({...})`.
/// Whether a fragment should ever produce *inlines* (so it can sit inside
/// `pandoc.Strong`) is an open design question; the spike always produces
/// blocks.
fn emit_fragment(markdown: &str, column: usize) -> Result<String, LmdError> {
    let markdown = markdown.trim();
    let blocks = parse_markdown(markdown)?;
    let mut emitter = Emitter::new(column);
    match blocks.as_slice() {
        [single] => emitter.block(single)?,
        many => emitter.blocks_call(many)?,
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
    fn simple_fragment_becomes_a_para() {
        let out = desugar_lua("return {= **hello** =}").unwrap();
        assert_eq!(
            out,
            r#"return pandoc.Para({
         pandoc.Strong({
           pandoc.Str("hello"),
         }),
       })"#
        );
    }

    #[test]
    fn inline_lmd_code_splices_lua_back_in() {
        let src = "return tmap(xs, function(v) return {= **`{lmd} v`** =} end)";
        let out = desugar_lua(src).unwrap();
        assert!(out.contains("pandoc.Strong({\n"), "{out}");
        assert!(out.contains("  v,\n"), "{out}");
        assert!(out.ends_with(" end)"), "{out}");
        assert!(!out.contains("{="), "{out}");
    }

    #[test]
    fn nested_fragment_inside_inline_lmd_code() {
        let src = "return {= *a* `{lmd} f({= b =})` =}";
        let out = desugar_lua(src).unwrap();
        assert!(out.contains("pandoc.Emph("), "{out}");
        assert!(out.contains("f(pandoc.Para({"), "{out}");
        assert!(out.contains("pandoc.Str(\"b\")"), "{out}");
        assert!(!out.contains("{="), "{out}");
    }

    #[test]
    fn block_lmd_cell_inside_fragment_becomes_an_iife() {
        let src = "return {=\nIntro\n\n```{lmd}\nreturn x\n```\n=}";
        let out = desugar_lua(src).unwrap();
        assert!(out.starts_with("return pandoc.Blocks({\n"), "{out}");
        assert!(out.contains("(function()\n"), "{out}");
        assert!(out.contains("return x\n"), "{out}");
        assert!(out.contains("end)(),\n"), "{out}");
    }

    #[test]
    fn empty_fragment_is_empty_blocks() {
        assert_eq!(
            desugar_lua("return {==}").unwrap(),
            "return pandoc.Blocks({})"
        );
    }

    #[test]
    fn several_fragments_in_one_chunk() {
        let out = desugar_lua("local a = {= x =}\nlocal b = {= y =}").unwrap();
        assert_eq!(out.matches("pandoc.Para(").count(), 2);
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
        let src = "return {= | a | b |\n|---|---|\n| 1 | 2 | =}";
        let err = desugar_lua(src).unwrap_err();
        assert!(matches!(err, LmdError::Unsupported("Table")), "{err}");
    }
}
