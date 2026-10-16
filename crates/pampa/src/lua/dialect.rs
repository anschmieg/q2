/*
 * lua/dialect.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Source dialects: languages that compile to Lua and that the scoped
//! `require` loads transparently (bd-76ogq4ew).
//!
//! A dialect is registered on a Lua state by the host (q2 registers lmd;
//! pampa on its own knows none). Once registered:
//!
//! - the scoped `require` (`quarto_api::register_scoped_require`) tries
//!   `<name>.<ext>` ahead of `<name>.lua` for every registered extension,
//!   compiles a hit through [`SourceDialect::compile`] before `load`, and
//!   warns when a `.lua` file of the same stem sits beside it (the dialect
//!   source is the one that loads; a stale compiled copy must not shadow
//!   the file the author is editing);
//! - `quarto.<name>.compile(source, chunkname)` is available to Lua, so a
//!   script can compile dialect text it holds (a fragment from a data
//!   file, say) and `load` the result itself.
//!
//! The compile step is the dialect's only job. Where the file comes from,
//! and whether it may be read, is decided by the [`SystemRuntime`] the
//! `require` reads through, so the same loader serves native, the WASM
//! virtual filesystem, and a sandboxed runtime with a read policy.
//!
//! The trait itself lives in [`crate::dialect`], outside the Lua feature,
//! so the filter pipeline can carry a dialect list whether or not Lua is
//! compiled in; this module is the Lua-side registration.
//!
//! [`SystemRuntime`]: quarto_system_runtime::SystemRuntime

use std::path::Path;
use std::sync::Arc;

use mlua::{Lua, Result, Table, Value};

pub use crate::dialect::{DialectError, SourceDialect};

/// The dialects registered on a Lua state, in registration order. Lives
/// in mlua's app data, so it is private to the state and not visible from
/// Lua.
#[derive(Default, Clone)]
struct Dialects(Vec<Arc<dyn SourceDialect>>);

/// Register a dialect on `lua`: the scoped `require` starts loading its
/// extension, and `quarto.<name>.compile` appears. Registering the same
/// extension twice replaces the earlier dialect.
pub fn register_source_dialect(lua: &Lua, dialect: Arc<dyn SourceDialect>) -> Result<()> {
    {
        let mut dialects = match lua.app_data_mut::<Dialects>() {
            Some(d) => d,
            None => {
                lua.set_app_data(Dialects::default());
                lua.app_data_mut::<Dialects>()
                    .expect("app data was just set")
            }
        };
        dialects.0.retain(|d| d.extension() != dialect.extension());
        dialects.0.push(dialect.clone());
    }

    let quarto: Table = lua.globals().get("quarto")?;
    let ns: Table = match quarto.get::<Value>(dialect.name())? {
        Value::Table(t) => t,
        _ => {
            let t = lua.create_table()?;
            quarto.set(dialect.name(), t.clone())?;
            t
        }
    };
    let compile_dialect = dialect.clone();
    let compile = lua.create_function(move |_, (source, name): (String, Option<String>)| {
        let name = name.unwrap_or_else(|| format!("<{}>", compile_dialect.name()));
        compile_dialect
            .compile(&source, Path::new(&name))
            .map_err(|e| mlua::Error::RuntimeError(format!("{name}: {}", e.message)))
    })?;
    ns.set("compile", compile)?;
    Ok(())
}

/// The dialects registered on `lua`, in registration order; empty when
/// none is.
pub(crate) fn registered_dialects(lua: &Lua) -> Vec<Arc<dyn SourceDialect>> {
    lua.app_data_ref::<Dialects>()
        .map(|d| d.0.clone())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A toy dialect for loader tests: `.shout` files are Lua in which the
    //! word `SHOUT` is a statement that returns the upper-cased string after
    //! it, so `SHOUT "hello"` compiles to `return string.upper("hello")`.
    //! A line containing `BOOM` does not compile.

    use super::*;

    pub struct Shout;

    impl SourceDialect for Shout {
        fn name(&self) -> &str {
            "shout"
        }
        fn extension(&self) -> &str {
            "shout"
        }
        fn compile(&self, source: &str, path: &Path) -> std::result::Result<String, DialectError> {
            if let Some(line) = source.lines().position(|l| l.contains("BOOM")) {
                return Err(DialectError::new(format!(
                    "{}:{}: BOOM is not a word",
                    path.display(),
                    line + 1
                )));
            }
            Ok(source.replace("SHOUT ", "return string.upper("))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::Shout;
    use super::*;
    use crate::lua::constructors::register_pandoc_namespace;
    use crate::lua::mediabag::create_shared_mediabag;
    use crate::lua::quarto_api::register_quarto_api;
    use quarto_system_runtime::NativeRuntime;

    fn lua_with_quarto() -> Lua {
        let lua = Lua::new();
        register_pandoc_namespace(
            &lua,
            Arc::new(NativeRuntime::new()),
            create_shared_mediabag(),
        )
        .unwrap();
        register_quarto_api(&lua).unwrap();
        lua
    }

    #[test]
    fn no_dialects_until_one_is_registered() {
        let lua = lua_with_quarto();
        assert!(registered_dialects(&lua).is_empty());
        register_source_dialect(&lua, Arc::new(Shout)).unwrap();
        let ds = registered_dialects(&lua);
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].extension(), "shout");
    }

    #[test]
    fn registering_the_same_extension_twice_keeps_one() {
        let lua = lua_with_quarto();
        register_source_dialect(&lua, Arc::new(Shout)).unwrap();
        register_source_dialect(&lua, Arc::new(Shout)).unwrap();
        assert_eq!(registered_dialects(&lua).len(), 1);
    }

    #[test]
    fn compile_is_exposed_under_quarto_name() {
        let lua = lua_with_quarto();
        register_source_dialect(&lua, Arc::new(Shout)).unwrap();
        let compiled: String = lua
            .load(r#"return quarto.shout.compile('SHOUT "hi")', "x.shout")"#)
            .eval()
            .unwrap();
        assert_eq!(compiled, r#"return string.upper("hi")"#);
        // The result is plain Lua a script can load itself.
        let value: String = lua
            .load(r#"return load(quarto.shout.compile('SHOUT "hi")'))()"#)
            .eval()
            .unwrap();
        assert_eq!(value, "HI");
    }

    #[test]
    fn compile_failure_is_a_lua_error_naming_the_chunk() {
        let lua = lua_with_quarto();
        register_source_dialect(&lua, Arc::new(Shout)).unwrap();
        let err = lua
            .load(r#"return quarto.shout.compile("BOOM", "data.shout")"#)
            .eval::<String>()
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("data.shout: data.shout:1: BOOM is not a word"),
            "{err}"
        );
    }
}
