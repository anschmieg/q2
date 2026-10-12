/*
 * lmd/emit.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pandoc AST → Lua `pandoc.*` constructor source.
//!
//! The output is meant to be read by a person first and run by a Lua
//! interpreter second: one constructor per line, lists broken across lines,
//! two-space indentation relative to the column the fragment started at.
//!
//! Call shapes follow pandoc's Lua API (and pampa's implementation of it in
//! `pampa::lua::constructors`): `pandoc.Link(content, target, title, attr)`,
//! `pandoc.Header(level, content, attr)`, and so on. An empty `Attr` is
//! omitted since every constructor's `attr` is optional.
//!
//! lmd code inside the markdown is the escape hatch back to Lua:
//!
//! - `` `{lmd} expr` `` (a `Code` inline) becomes `expr`, desugared
//!   recursively, in the enclosing inline list.
//! - `` ```{lmd} `` (a `CodeBlock`) becomes `(function() body end)()`, body
//!   desugared recursively, in the enclosing block list.

use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{CitationMode, Inline, MathType, QuoteType};
use quarto_pandoc_types::list::{ListNumberDelim, ListNumberStyle};

use super::desugar::{LmdError, desugar_lua_at, desugar_lua_expr};
use crate::engine::capture_splice::engine_cell_lang;

/// Output buffer plus indentation state.
pub(super) struct Emitter {
    out: String,
    /// Column of the `{=` this fragment replaces; continuation lines are
    /// indented past it so the splice reads as one expression.
    base: usize,
    depth: usize,
}

impl Emitter {
    pub(super) fn new(base: usize) -> Self {
        Self {
            out: String::new(),
            base,
            depth: 0,
        }
    }

    pub(super) fn finish(self) -> String {
        self.out
    }

    fn indent(&self) -> String {
        " ".repeat(self.base + 2 * self.depth)
    }

    fn push(&mut self, s: &str) {
        self.out.push_str(s);
    }

    /// Write `{` item `,` … `}` with one item per line, or `{}` when empty.
    fn list<T>(
        &mut self,
        items: &[T],
        mut each: impl FnMut(&mut Self, &T) -> Result<(), LmdError>,
    ) -> Result<(), LmdError> {
        if items.is_empty() {
            self.push("{}");
            return Ok(());
        }
        self.push("{\n");
        self.depth += 1;
        for item in items {
            let indent = self.indent();
            self.push(&indent);
            each(self, item)?;
            self.push(",\n");
        }
        self.depth -= 1;
        let indent = self.indent();
        self.push(&indent);
        self.push("}");
        Ok(())
    }

    /// `pandoc.Name(` … `)` where the closure writes the arguments.
    fn call(
        &mut self,
        name: &str,
        args: impl FnOnce(&mut Self) -> Result<(), LmdError>,
    ) -> Result<(), LmdError> {
        self.push("pandoc.");
        self.push(name);
        self.push("(");
        args(self)?;
        self.push(")");
        Ok(())
    }

    fn string(&mut self, s: &str) {
        self.out.push_str(&lua_string_literal(s));
    }

    /// `, pandoc.Attr(...)` when the attr is non-empty, nothing otherwise.
    fn attr_arg(&mut self, attr: &Attr) -> Result<(), LmdError> {
        if quarto_pandoc_types::attr::is_empty_attr(attr) {
            return Ok(());
        }
        self.push(", ");
        self.attr(attr)
    }

    fn attr(&mut self, attr: &Attr) -> Result<(), LmdError> {
        let (id, classes, kvs) = attr;
        let kvs: Vec<(&String, &String)> = kvs.iter().collect();
        self.call("Attr", |e| {
            e.string(id);
            e.push(", ");
            e.list(classes, |e, c| {
                e.string(c);
                Ok(())
            })?;
            e.push(", ");
            e.list(&kvs, |e, (k, v)| {
                e.push("{");
                e.string(k);
                e.push(", ");
                e.string(v);
                e.push("}");
                Ok(())
            })
        })
    }

    /// A Lua chunk spliced in from an lmd cell inside the markdown. Nested
    /// fragments inside it are desugared at this emitter's current indent.
    fn splice_lua_chunk(&mut self, src: &str) -> Result<String, LmdError> {
        desugar_lua_at(src, self.base + 2 * self.depth)
    }

    /// A Lua expression spliced in from inline lmd code inside the markdown.
    fn splice_lua_expr(&mut self, src: &str) -> Result<String, LmdError> {
        desugar_lua_expr(src, self.base + 2 * self.depth)
    }

    // ----- blocks -------------------------------------------------------

    pub(super) fn blocks(&mut self, blocks: &[Block]) -> Result<(), LmdError> {
        self.list(blocks, |e, b| e.block(b))
    }

    /// `pandoc.Blocks({...})`: the form a multi-block fragment takes.
    pub(super) fn blocks_call(&mut self, blocks: &[Block]) -> Result<(), LmdError> {
        self.call("Blocks", |e| e.blocks(blocks))
    }

    pub(super) fn block(&mut self, block: &Block) -> Result<(), LmdError> {
        // lmd escape hatch: a `{lmd}` cell inside a fragment is Lua whose
        // value stands in for the block.
        if let Block::CodeBlock(cb) = block
            && engine_cell_lang(block) == Some(super::LMD_LANG)
        {
            let body = self.splice_lua_chunk(cb.text.trim_end())?;
            let inner = " ".repeat(self.base + 2 * (self.depth + 1));
            let outer = self.indent();
            self.push("(function()\n");
            for line in body.lines() {
                self.push(&inner);
                self.push(line);
                self.push("\n");
            }
            self.push(&outer);
            self.push("end)()");
            return Ok(());
        }
        match block {
            Block::Plain(b) => self.call("Plain", |e| e.inlines(&b.content)),
            Block::Paragraph(b) => self.call("Para", |e| e.inlines(&b.content)),
            Block::LineBlock(b) => self.call("LineBlock", |e| {
                e.list(&b.content, |e, line| e.inlines(line))
            }),
            Block::CodeBlock(b) => self.call("CodeBlock", |e| {
                e.string(&b.text);
                e.attr_arg(&b.attr)
            }),
            Block::RawBlock(b) => self.call("RawBlock", |e| {
                e.string(&b.format);
                e.push(", ");
                e.string(&b.text);
                Ok(())
            }),
            Block::BlockQuote(b) => self.call("BlockQuote", |e| e.blocks(&b.content)),
            Block::OrderedList(b) => self.call("OrderedList", |e| {
                e.list(&b.content, |e, item| e.blocks(item))?;
                let (start, style, delim) = &b.attr;
                e.push(", ");
                e.call("ListAttributes", |e| {
                    e.push(&start.to_string());
                    e.push(", ");
                    e.string(list_number_style(style));
                    e.push(", ");
                    e.string(list_number_delim(delim));
                    Ok(())
                })
            }),
            Block::BulletList(b) => self.call("BulletList", |e| {
                e.list(&b.content, |e, item| e.blocks(item))
            }),
            Block::DefinitionList(b) => self.call("DefinitionList", |e| {
                e.list(&b.content, |e, (term, defs)| {
                    e.push("{");
                    e.inlines(term)?;
                    e.push(", ");
                    e.list(defs, |e, d| e.blocks(d))?;
                    e.push("}");
                    Ok(())
                })
            }),
            Block::Header(b) => self.call("Header", |e| {
                e.push(&b.level.to_string());
                e.push(", ");
                e.inlines(&b.content)?;
                e.attr_arg(&b.attr)
            }),
            Block::HorizontalRule(_) => self.call("HorizontalRule", |_| Ok(())),
            Block::Div(b) => self.call("Div", |e| {
                e.blocks(&b.content)?;
                e.attr_arg(&b.attr)
            }),
            Block::Figure(b) => self.call("Figure", |e| {
                e.blocks(&b.content)?;
                e.push(", {");
                if let Some(long) = &b.caption.long {
                    e.push("long = ");
                    e.blocks(long)?;
                }
                if let Some(short) = &b.caption.short {
                    if b.caption.long.is_some() {
                        e.push(", ");
                    }
                    e.push("short = ");
                    e.inlines(short)?;
                }
                e.push("}");
                e.attr_arg(&b.attr)
            }),
            Block::Table(_) => Err(LmdError::Unsupported("Table")),
            Block::BlockMetadata(_) => Err(LmdError::Unsupported("BlockMetadata")),
            Block::NoteDefinitionPara(_) => Err(LmdError::Unsupported("NoteDefinitionPara")),
            Block::NoteDefinitionFencedBlock(_) => {
                Err(LmdError::Unsupported("NoteDefinitionFencedBlock"))
            }
            Block::CaptionBlock(_) => Err(LmdError::Unsupported("CaptionBlock")),
            Block::Custom(_) => Err(LmdError::Unsupported("Custom")),
        }
    }

    // ----- inlines ------------------------------------------------------

    pub(super) fn inlines(&mut self, inlines: &[Inline]) -> Result<(), LmdError> {
        self.list(inlines, |e, i| e.inline(i))
    }

    fn inline(&mut self, inline: &Inline) -> Result<(), LmdError> {
        // lmd escape hatch: `{lmd} expr` is Lua whose value stands in for
        // the inline.
        if let Inline::Code(code) = inline
            && let Some(expr) = inline_lmd_source(&code.attr, &code.text)
        {
            let lua = self.splice_lua_expr(expr)?;
            self.push(&lua);
            return Ok(());
        }
        match inline {
            Inline::Str(s) => self.call("Str", |e| {
                e.string(&s.text);
                Ok(())
            }),
            Inline::Emph(i) => self.call("Emph", |e| e.inlines(&i.content)),
            Inline::Underline(i) => self.call("Underline", |e| e.inlines(&i.content)),
            Inline::Strong(i) => self.call("Strong", |e| e.inlines(&i.content)),
            Inline::Strikeout(i) => self.call("Strikeout", |e| e.inlines(&i.content)),
            Inline::Superscript(i) => self.call("Superscript", |e| e.inlines(&i.content)),
            Inline::Subscript(i) => self.call("Subscript", |e| e.inlines(&i.content)),
            Inline::SmallCaps(i) => self.call("SmallCaps", |e| e.inlines(&i.content)),
            Inline::Quoted(i) => self.call("Quoted", |e| {
                e.string(match i.quote_type {
                    QuoteType::SingleQuote => "SingleQuote",
                    QuoteType::DoubleQuote => "DoubleQuote",
                });
                e.push(", ");
                e.inlines(&i.content)
            }),
            Inline::Cite(i) => self.call("Cite", |e| {
                e.inlines(&i.content)?;
                e.push(", ");
                e.list(&i.citations, |e, c| {
                    e.call("Citation", |e| {
                        e.string(&c.id);
                        e.push(", ");
                        e.string(match c.mode {
                            CitationMode::AuthorInText => "AuthorInText",
                            CitationMode::SuppressAuthor => "SuppressAuthor",
                            CitationMode::NormalCitation => "NormalCitation",
                        });
                        e.push(", ");
                        e.inlines(&c.prefix)?;
                        e.push(", ");
                        e.inlines(&c.suffix)?;
                        e.push(&format!(", {}, {}", c.note_num, c.hash));
                        Ok(())
                    })
                })
            }),
            Inline::Code(i) => self.call("Code", |e| {
                e.string(&i.text);
                e.attr_arg(&i.attr)
            }),
            Inline::Space(_) => self.call("Space", |_| Ok(())),
            Inline::SoftBreak(_) => self.call("SoftBreak", |_| Ok(())),
            Inline::LineBreak(_) => self.call("LineBreak", |_| Ok(())),
            Inline::Math(i) => self.call("Math", |e| {
                e.string(match i.math_type {
                    MathType::InlineMath => "InlineMath",
                    MathType::DisplayMath => "DisplayMath",
                });
                e.push(", ");
                e.string(&i.text);
                Ok(())
            }),
            Inline::RawInline(i) => self.call("RawInline", |e| {
                e.string(&i.format);
                e.push(", ");
                e.string(&i.text);
                Ok(())
            }),
            Inline::Link(i) => self.call("Link", |e| {
                e.inlines(&i.content)?;
                e.push(", ");
                e.string(&i.target.0);
                e.push(", ");
                e.string(&i.target.1);
                e.attr_arg(&i.attr)
            }),
            Inline::Image(i) => self.call("Image", |e| {
                e.inlines(&i.content)?;
                e.push(", ");
                e.string(&i.target.0);
                e.push(", ");
                e.string(&i.target.1);
                e.attr_arg(&i.attr)
            }),
            Inline::Note(i) => self.call("Note", |e| e.blocks(&i.content)),
            Inline::Span(i) => self.call("Span", |e| {
                e.inlines(&i.content)?;
                e.attr_arg(&i.attr)
            }),
            Inline::Shortcode(_) => Err(LmdError::Unsupported("Shortcode")),
            Inline::NoteReference(_) => Err(LmdError::Unsupported("NoteReference")),
            Inline::Attr(_) => Err(LmdError::Unsupported("Attr")),
            Inline::Insert(_) => Err(LmdError::Unsupported("Insert")),
            Inline::Delete(_) => Err(LmdError::Unsupported("Delete")),
            Inline::Highlight(_) => Err(LmdError::Unsupported("Highlight")),
            Inline::EditComment(_) => Err(LmdError::Unsupported("EditComment")),
            Inline::Custom(_) => Err(LmdError::Unsupported("Custom")),
        }
    }
}

/// The Lua source of an inline lmd code span, if `attr`/`text` describe one.
///
/// The qmd reader turns both `` `{lmd} expr` `` and `` `expr`{lmd} `` into a
/// `Code` whose text is `{lmd} expr`; a `{lmd}` class is accepted too in
/// case a filter built the node.
fn inline_lmd_source<'a>(attr: &Attr, text: &'a str) -> Option<&'a str> {
    let tag = format!("{{{}}}", super::LMD_LANG);
    if let Some(rest) = text.strip_prefix(&tag) {
        return Some(rest.trim());
    }
    if attr.1.iter().any(|c| c == &tag) {
        return Some(text.trim());
    }
    None
}

fn list_number_style(style: &ListNumberStyle) -> &'static str {
    match style {
        ListNumberStyle::Default => "DefaultStyle",
        ListNumberStyle::Example => "Example",
        ListNumberStyle::Decimal => "Decimal",
        ListNumberStyle::LowerRoman => "LowerRoman",
        ListNumberStyle::UpperRoman => "UpperRoman",
        ListNumberStyle::LowerAlpha => "LowerAlpha",
        ListNumberStyle::UpperAlpha => "UpperAlpha",
    }
}

fn list_number_delim(delim: &ListNumberDelim) -> &'static str {
    match delim {
        ListNumberDelim::Default => "DefaultDelim",
        ListNumberDelim::Period => "Period",
        ListNumberDelim::OneParen => "OneParen",
        ListNumberDelim::TwoParens => "TwoParens",
    }
}

/// A double-quoted Lua string literal for `s`.
pub(super) fn lua_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\x7f' => {
                out.push_str(&format!("\\{:03}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_literal_escapes() {
        assert_eq!(lua_string_literal("plain"), "\"plain\"");
        assert_eq!(lua_string_literal("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(
            lua_string_literal("line\nbreak\ttab"),
            "\"line\\nbreak\\ttab\""
        );
        assert_eq!(lua_string_literal("\u{1}"), "\"\\001\"");
        assert_eq!(lua_string_literal("héllo"), "\"héllo\"");
    }

    #[test]
    fn inline_lmd_source_recognizes_both_forms() {
        let empty: Attr = (String::new(), vec![], Default::default());
        assert_eq!(inline_lmd_source(&empty, "{lmd} v"), Some("v"));
        assert_eq!(inline_lmd_source(&empty, "{lmd}v + 1"), Some("v + 1"));
        assert_eq!(inline_lmd_source(&empty, "{python} v"), None);
        assert_eq!(inline_lmd_source(&empty, "code"), None);
        let classed: Attr = (String::new(), vec!["{lmd}".into()], Default::default());
        assert_eq!(inline_lmd_source(&classed, " v "), Some("v"));
    }
}
