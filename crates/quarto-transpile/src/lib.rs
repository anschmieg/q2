/*
 * quarto-transpile/src/lib.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * TypeScript / JSX to JavaScript for Quarto components (oxc).
 */

//! Transpile a `.tsx` / `.ts` / `.jsx` module to browser JavaScript.
//!
//! One file in, one ES module out: types are stripped, JSX is compiled
//! with the automatic runtime (`react/jsx-runtime`, resolved by the
//! page's import map), relative import specifiers that name a
//! TypeScript extension are rewritten to `.js`, and nothing is bundled.
//! Pure Rust (oxc), so the same function serves the native renderer and
//! the browser-side WASM. bd-react-components-a8flgldm, plan D7 option B.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{
    JsxOptions, JsxRuntime, RewriteExtensionsMode, TransformOptions, Transformer, TypeScriptOptions,
};

/// A transpilation problem, located in the source when oxc gives a span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranspileError {
    pub message: String,
    /// 1-based line and column of the first label, when known.
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl std::fmt::Display for TranspileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "{}:{}: {}", l, c, self.message),
            _ => write!(f, "{}", self.message),
        }
    }
}

/// The source type oxc infers from a file name; `None` for a file that is
/// not JavaScript or TypeScript.
pub fn source_type_for(file_name: &str) -> Option<SourceType> {
    SourceType::from_path(Path::new(file_name)).ok()
}

/// Is this a file the transpiler must process before a browser can load
/// it (TypeScript, or JSX in a `.jsx` file)?
pub fn needs_transpile(file_name: &str) -> bool {
    matches!(
        Path::new(file_name).extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx" | "mts" | "cts" | "jsx")
    )
}

/// A transpiled module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transpiled {
    /// The ES module source.
    pub code: String,
    /// Relative import specifiers (`./x.js`, `../y.ts`) as written in the
    /// source, before extension rewriting, in source order without
    /// duplicates. Bare and absolute specifiers are not listed.
    pub relative_imports: Vec<String>,
}

/// The relative import specifiers of a JavaScript or TypeScript module,
/// without transforming it (for `.js` files that ship verbatim). An
/// unparsable file yields an empty list: it will fail in the browser with
/// its own message, and the transform ships what it was given.
pub fn relative_imports_of(source: &str, file_name: &str) -> Vec<String> {
    let Some(source_type) = source_type_for(file_name) else {
        return Vec::new();
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Vec::new();
    }
    relative_imports(&parsed.module_record)
}

fn relative_imports(record: &oxc_syntax::module_record::ModuleRecord<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (specifier, _) in record.requested_modules.iter() {
        let spec = specifier.as_str();
        if (spec.starts_with("./") || spec.starts_with("../")) && !out.iter().any(|s| s == spec) {
            out.push(spec.to_string());
        }
    }
    out
}

/// Transpile `source` (named `file_name`, whose extension picks the
/// dialect) to an ES module. Errors are every parse, semantic and
/// transform diagnostic oxc reports, in order.
pub fn transpile(source: &str, file_name: &str) -> Result<Transpiled, Vec<TranspileError>> {
    let source_type = source_type_for(file_name).ok_or_else(|| {
        vec![TranspileError {
            message: format!("`{file_name}` is not a JavaScript or TypeScript file"),
            line: None,
            column: None,
        }]
    })?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(convert(source, parsed.diagnostics));
    }
    let mut program = parsed.program;
    let relative_imports = relative_imports(&parsed.module_record);

    let semantic = SemanticBuilder::new().build(&program);
    if !semantic.diagnostics.is_empty() {
        return Err(convert(source, semantic.diagnostics));
    }
    let scoping = semantic.semantic.into_scoping();

    let options = TransformOptions {
        typescript: TypeScriptOptions {
            rewrite_import_extensions: Some(RewriteExtensionsMode::Rewrite),
            ..TypeScriptOptions::default()
        },
        jsx: JsxOptions {
            runtime: JsxRuntime::Automatic,
            import_source: Some("react".to_string()),
            ..JsxOptions::default()
        },
        ..TransformOptions::default()
    };
    let transformed = Transformer::new(&allocator, Path::new(file_name), &options)
        .build_with_scoping(scoping, &mut program);
    if !transformed.diagnostics.is_empty() {
        return Err(convert(source, transformed.diagnostics));
    }

    Ok(Transpiled {
        code: Codegen::new().build(&program).code,
        relative_imports,
    })
}

fn convert(
    source: &str,
    errors: impl IntoIterator<Item = oxc_diagnostics::OxcDiagnostic>,
) -> Vec<TranspileError> {
    errors
        .into_iter()
        .map(|d| {
            let offset = d.labels.first().map(|l| l.offset());
            let (line, column) = match offset {
                Some(o) => {
                    let (l, c) = line_col(source, o as usize);
                    (Some(l), Some(c))
                }
                None => (None, None),
            };
            TranspileError {
                message: d.message.to_string(),
                line,
                column,
            }
        })
        .collect()
}

fn line_col(source: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() as u32 + 1;
    let column = before.rsplit('\n').next().map_or(0, |s| s.chars().count()) as u32 + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsx_becomes_js_with_the_automatic_jsx_runtime() {
        let src = r#"
import { useState } from "react";
import type { QuartoComponentProps } from "@quarto/components";
export default function Counter({ children }: QuartoComponentProps<{ start?: number }>) {
  const [n, setN] = useState<number>(0);
  return <div className="c" onClick={() => setN(n + 1)}>{n}{children}</div>;
}
"#;
        let out = transpile(src, "counter.tsx").expect("transpiles").code;
        assert!(out.contains("from \"react/jsx-runtime\""), "{out}");
        assert!(out.contains("_jsxs(\"div\""), "{out}");
        assert!(!out.contains("QuartoComponentProps"), "types erased: {out}");
        assert!(!out.contains("<number>"), "{out}");
        assert!(out.contains("import { useState } from \"react\""), "{out}");
    }

    #[test]
    fn relative_ts_imports_are_rewritten_to_js() {
        let t = transpile(
            "import { a } from \"./util.ts\";\nimport r from \"react\";\nexport const b = a;\n",
            "x.ts",
        )
        .unwrap();
        let out = t.code;
        assert_eq!(t.relative_imports, vec!["./util.ts".to_string()]);
        assert!(out.contains("\"./util.js\""), "{out}");
    }

    #[test]
    fn plain_jsx_file_compiles_too() {
        let out = transpile("export const X = () => <b>hi</b>;\n", "x.jsx")
            .unwrap()
            .code;
        assert!(out.contains("_jsx(\"b\""), "{out}");
    }

    #[test]
    fn relative_imports_of_a_plain_js_module() {
        let src = "import a from \"./a.js\";\nimport { b } from \"../b/index.js\";\nimport c from \"c\";\nexport * from \"./a.js\";\nexport const d = [a, b, c];\n";
        assert_eq!(
            relative_imports_of(src, "m.js"),
            vec!["./a.js".to_string(), "../b/index.js".to_string()]
        );
        assert!(relative_imports_of("import {", "bad.js").is_empty());
        assert!(relative_imports_of("x", "a.css").is_empty());
    }

    #[test]
    fn syntax_error_is_located() {
        let errs = transpile("const = 1;\n", "bad.ts").unwrap_err();
        assert!(!errs.is_empty());
        assert_eq!(errs[0].line, Some(1));
        assert!(errs[0].column.is_some());
    }

    #[test]
    fn non_js_file_is_an_error_and_needs_transpile_is_by_extension() {
        assert!(transpile("x", "a.css").is_err());
        assert!(needs_transpile("a.tsx") && needs_transpile("a.ts") && needs_transpile("a.jsx"));
        assert!(!needs_transpile("a.js") && !needs_transpile("a.mjs"));
    }
}
