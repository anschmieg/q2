/*
 * lib.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Tree-sitter grammar for **lmd**: Lua 5.4 plus the `{= markdown =}`
//! fragment expression used by Quarto 2's "lua+md" experiment.
//!
//! The grammar is a fork of `tree-sitter-grammars/tree-sitter-lua` v0.5.0
//! with one addition: [`markdown_fragment`], an expression whose content is
//! raw markdown text that the caller hands to the qmd parser. The grammar is
//! named `lmd` (not `lua`) so this crate can coexist with the upstream
//! `tree-sitter-lua` crate that `quarto-highlight` links.
//!
//! See `claude-notes/plans/2026-10-12-lmd-exploration.md`.
//!
//! [`markdown_fragment`]: https://github.com/posit-dev/quarto-markdown-syntax/tree/main/crates/tree-sitter-lmd/grammar/grammar.js

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_lmd() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for the lmd grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_lmd) };

/// The content of the [`node-types.json`][] file for the lmd grammar.
///
/// [`node-types.json`]: https://tree-sitter.github.io/tree-sitter/using-parsers#static-node-types
pub const NODE_TYPES: &str = include_str!("../grammar/src/node-types.json");

/// Node kind of the `{= … =}` expression.
pub const MARKDOWN_FRAGMENT: &str = "markdown_fragment";

/// Node kind of the raw markdown text inside a [`MARKDOWN_FRAGMENT`]
/// (absent when the fragment is empty, `{==}`).
pub const MARKDOWN_CONTENT: &str = "markdown_content";

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("Error loading lmd grammar");
        parser.parse(src, None).expect("parse returned None")
    }

    #[test]
    fn can_load_grammar() {
        parse("return 42");
    }

    #[test]
    fn markdown_fragment_is_an_expression_with_raw_content() {
        let src = "return {= **hello** =}";
        let tree = parse(src);
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let mut cursor = tree.walk();
        let mut found = None;
        'outer: loop {
            let node = cursor.node();
            if node.kind() == MARKDOWN_FRAGMENT {
                found = Some(node);
                break;
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    break 'outer;
                }
            }
        }
        let fragment = found.expect("no markdown_fragment node");
        let content = fragment
            .child_by_field_name("content")
            .expect("fragment has content");
        assert_eq!(content.kind(), MARKDOWN_CONTENT);
        assert_eq!(content.utf8_text(src.as_bytes()).unwrap(), " **hello** ");
    }

    #[test]
    fn plain_tables_still_parse() {
        let tree = parse("local t = { 1, x = 2, [3] = 4 }\nlocal u = {}");
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
    }
}
