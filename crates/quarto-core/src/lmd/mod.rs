/*
 * lmd/mod.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! **lmd** ("lua+md"): a JSX-like mode where filter logic lives next to the
//! markdown it acts on. Exploration spike; see
//! `claude-notes/plans/2026-10-12-lmd-exploration.md` and braid strand
//! `bd-lmd-exploration-jqv2nbl4`.
//!
//! Two pieces of syntax:
//!
//! 1. An executable cell `` ```{lmd} `` whose body is Lua.
//! 2. Inside that Lua, a markdown fragment is an expression: the markdown
//!    is parsed with the qmd reader and rewritten into `pandoc.*`
//!    constructor calls. Three forms, differing only in how the result is
//!    typed:
//!
//!    | form | expands to |
//!    |---|---|
//!    | `{= … =}` | `pandoc.Nodes({ … })`, the context decides (not implemented yet) |
//!    | `{=: … :=}` | `pandoc.Blocks({ … })` |
//!    | `{=[ … ]=}` | `pandoc.Inlines({ … })` |
//!
//!    Markdown inside a fragment can itself contain lmd code
//!    (`` `{lmd} expr` `` inline, `` ```{lmd} `` as a block), which splices
//!    the Lua back in, so the two languages nest in either direction.
//!
//! This module is the desugaring half: [`desugar_lua`] turns lmd source
//! into plain Lua. The stage that applies it to a document
//! ([`crate::stage::stages::LmdDesugarStage`]) currently only *shows* the
//! result, replacing each `{lmd}` cell with a non-executable `lmd` code
//! block, so the desugaring can be inspected by eye before anything runs it.

mod desugar;
mod emit;

pub use desugar::{FragmentKind, LmdError, desugar_lua};

/// The language tag an lmd cell carries: `` ```{lmd} `` parses to a
/// `CodeBlock` whose first class is `{lmd}`, and `` `{lmd} expr` `` to a
/// `Code` whose text starts with `{lmd}`.
pub const LMD_LANG: &str = "lmd";
