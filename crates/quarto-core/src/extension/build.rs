/*
 * extension/build.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Build a TypeScript engine extension bundle (esbuild).
 */

//! Build logic for `q2 call build-ts-extension` — build a TypeScript engine
//! extension bundle with esbuild.
//!
//! Extension authors run this after editing TS source; `q2` never runs it
//! during render. It resolves an alias config by the precedence rules in
//! [`resolve_build_config`], locates an esbuild binary ([`resolve_esbuild_binary`]),
//! and spawns it directly — no Node and no Deno at build time.
//!
//! Native-only (mirrors `engine::ts_process`): spawns esbuild via
//! `std::process`, unavailable on `wasm32-unknown-unknown`.
//!
//! The bundle is ESM for `node >= 24`, with `node:*` imports left external.
//! The same file runs under q2's Node host and Quarto 1's Deno 2.x. Imports
//! that only exist under Deno (`jsr:`, `@std/*`) are not resolved: they fail
//! with a message naming the `node:` replacement.
//!
//! # Config precedence
//!
//! The config is a JSON file `{"alias": {"<import>": "<path>"}}`; relative
//! paths resolve against the config file's directory.
//!
//! 1. `--config <path>` (explicit override) — always wins.
//! 2. `esbuild.json` committed in the extension directory.
//! 3. Workspace — when `workspace_root` is `Some` (auto-detected or
//!    `--workspace`): `@quarto/api` and `@quarto/types` alias to the
//!    workspace's `ts-packages/*/src`, so extensions that import values from
//!    them (`@quarto/api/claims`) build from source.
//! 4. Shipped template — embedded at compile time (`SHIPPED_ESBUILD_JSON`).
//!    Installed binaries land here when tiers 1-3 are absent. It declares no
//!    aliases: `@quarto/*` is not yet published, so a *value* import of it
//!    fails with an actionable message (type-only imports are erased and
//!    need nothing).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Options for building an extension's TypeScript engine bundle.
#[derive(Debug)]
pub struct BuildOptions {
    /// Path to the extension directory or `_extension.yml`. Defaults to cwd.
    pub ext_dir: Option<PathBuf>,
    /// Explicit `--config` override; wins over all other config sources.
    pub config: Option<PathBuf>,
    /// Force use of the workspace tier (in-repo / pre-publish build).
    pub workspace: bool,
    /// Use this q2 workspace root for the workspace tier and for finding
    /// `node_modules/@esbuild`, instead of walking up from the extension
    /// (for extensions built outside the workspace, e.g. in a temp dir).
    pub workspace_root: Option<PathBuf>,
    /// Explicit esbuild binary; wins over every other way of finding one.
    pub esbuild_binary: Option<PathBuf>,
    /// Extracts an esbuild binary embedded in the `q2` executable and returns
    /// its path (`Ok(None)` for a placeholder build that embeds nothing).
    pub embedded_esbuild: Option<fn() -> Result<Option<PathBuf>>>,
}

// ============================================================================
// Config resolution
// ============================================================================

/// Which config tier [`resolve_build_config`] selected. Pure data: no I/O
/// happens until [`load_aliases`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Tier 1/2: a user-written `esbuild.json` (explicit or extension-local).
    File(PathBuf),
    /// Tier 3: aliases derived from this q2 workspace root.
    Workspace(PathBuf),
    /// Tier 4: the embedded shipped template.
    Shipped,
}

/// Resolves which alias config applies. Pure: callers probe the filesystem
/// for tiers 1-3 and pass the results.
///
/// 1. `explicit` wins unconditionally.
/// 2. `ext_esbuild_json` — `Some` when an `esbuild.json` exists in the
///    extension directory.
/// 3. `workspace_root` — `Some` when a q2 workspace was detected (or
///    `--workspace` forced).
/// 4. Otherwise the shipped template.
pub fn resolve_build_config(
    explicit: Option<&Path>,
    ext_esbuild_json: Option<&Path>,
    workspace_root: Option<&Path>,
) -> ConfigSource {
    if let Some(p) = explicit {
        return ConfigSource::File(p.to_path_buf());
    }
    if let Some(p) = ext_esbuild_json {
        return ConfigSource::File(p.to_path_buf());
    }
    if let Some(root) = workspace_root {
        return ConfigSource::Workspace(root.to_path_buf());
    }
    ConfigSource::Shipped
}

/// The shipped `resources/extension-build/esbuild.json` template (repo root),
/// embedded at compile time. The release tarball is single-member (just the
/// `q2` binary), so there is no adjacent `resources/` directory to probe.
const SHIPPED_ESBUILD_JSON: &str =
    include_str!("../../../../resources/extension-build/esbuild.json");

/// The on-disk shape of an `esbuild.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EsbuildConfigFile {
    #[serde(default)]
    alias: BTreeMap<String, String>,
}

/// Parse an `esbuild.json`; relative alias targets resolve against `base`.
fn parse_config(text: &str, base: &Path, what: &str) -> Result<BTreeMap<String, PathBuf>> {
    let file: EsbuildConfigFile =
        serde_json::from_str(text).with_context(|| format!("Invalid {what}"))?;
    Ok(file
        .alias
        .into_iter()
        .map(|(name, target)| {
            let target = PathBuf::from(target);
            let target = if target.is_absolute() {
                target
            } else {
                base.join(target)
            };
            (name, target)
        })
        .collect())
}

/// Read the import aliases a [`ConfigSource`] stands for.
pub fn load_aliases(source: &ConfigSource) -> Result<BTreeMap<String, PathBuf>> {
    match source {
        ConfigSource::File(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let base = path.parent().unwrap_or_else(|| Path::new("."));
            parse_config(&text, base, &path.display().to_string())
        }
        ConfigSource::Workspace(root) => {
            let src = |pkg: &str| root.join("ts-packages").join(pkg).join("src");
            Ok(BTreeMap::from([
                ("@quarto/api".to_string(), src("quarto-api")),
                ("@quarto/types".to_string(), src("quarto-types")),
            ]))
        }
        ConfigSource::Shipped => {
            parse_config(SHIPPED_ESBUILD_JSON, Path::new("."), "shipped esbuild.json")
        }
    }
}

// ============================================================================
// Workspace detection
// ============================================================================

/// Walk up from `start_dir` looking for a directory that contains
/// `ts-packages/quarto-api` (the marker for a q2 source clone).
///
/// Returns `None` when no such directory is found (i.e. we are in an
/// installed-binary context where the packages are not present).
pub fn find_workspace_root(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
    loop {
        if dir.join("ts-packages").join("quarto-api").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ============================================================================
// esbuild binary
// ============================================================================

/// The `@esbuild/<name>` platform package and the binary's path inside it
/// for the current target, or `None` on a platform esbuild does not ship for.
fn esbuild_platform_package() -> Option<(&'static str, &'static str)> {
    let name = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "win32-x64",
        ("windows", "aarch64") => "win32-arm64",
        _ => return None,
    };
    // The Windows package keeps the binary at its root; the others use bin/.
    let bin = if cfg!(windows) {
        "esbuild.exe"
    } else {
        "bin/esbuild"
    };
    Some((name, bin))
}

/// Walk up from `start` for `node_modules/@esbuild/<platform>/<bin>`.
fn find_node_modules_esbuild(start: &Path) -> Option<PathBuf> {
    let (pkg, bin) = esbuild_platform_package()?;
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir
            .join("node_modules")
            .join("@esbuild")
            .join(pkg)
            .join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Locate the esbuild binary. In order: `BuildOptions.esbuild_binary`, the
/// `ESBUILD_BINARY_PATH` environment variable, the binary embedded in this
/// executable, then the workspace's `node_modules/@esbuild/<platform>`
/// (what `npm ci` installs; the route tests take).
pub fn resolve_esbuild_binary(
    opts: &BuildOptions,
    ext_dir: &Path,
    workspace_root: Option<&Path>,
) -> Result<PathBuf> {
    // The binary runs with `current_dir = ext_dir`, where a relative program
    // path would resolve against the wrong directory; always hand back an
    // absolute path.
    let absolute = |p: PathBuf| std::path::absolute(&p).unwrap_or(p);
    if let Some(p) = &opts.esbuild_binary {
        return Ok(absolute(p.clone()));
    }
    if let Some(p) = std::env::var_os("ESBUILD_BINARY_PATH").filter(|p| !p.is_empty()) {
        return Ok(absolute(PathBuf::from(p)));
    }
    if let Some(extract) = opts.embedded_esbuild
        && let Some(p) = extract()?
    {
        return Ok(p);
    }
    if let Some(p) = find_node_modules_esbuild(ext_dir)
        .or_else(|| workspace_root.and_then(find_node_modules_esbuild))
    {
        return Ok(absolute(p));
    }
    bail!(
        "No esbuild binary found. This q2 build does not embed one \
         (run `cargo xtask fetch-esbuild` before building q2), and \
         node_modules/@esbuild/<platform> is not reachable from {} \
         (run `npm ci` in the q2 workspace). Set ESBUILD_BINARY_PATH to use another esbuild.",
        ext_dir.display()
    )
}

// ============================================================================
// Command handler
// ============================================================================

/// Builds the extension's `dist/*.js` bundle; returns the resolved output path.
pub fn build_ts_extension(opts: BuildOptions) -> Result<PathBuf> {
    // Locate the extension directory (containing _extension.yml).
    let ext_dir = resolve_extension_dir(opts.ext_dir.as_deref())?;

    // Auto-detect workspace root (walk up from ext_dir).
    let workspace_root = if let Some(root) = &opts.workspace_root {
        Some(root.clone())
    } else if opts.workspace {
        // --workspace: walk up unconditionally.
        Some(find_workspace_root(&ext_dir).with_context(|| {
            format!(
                "--workspace was passed but no q2 workspace root (dir containing \
                         ts-packages/quarto-api) was found walking up from {}",
                ext_dir.display()
            )
        })?)
    } else {
        find_workspace_root(&ext_dir)
    };

    // Probe for an extension-local esbuild.json. A leftover deno.json is no
    // longer read; say so rather than silently ignoring it.
    let ext_esbuild = Some(ext_dir.join("esbuild.json")).filter(|p| p.exists());
    if ext_esbuild.is_none() && opts.config.is_none() && ext_dir.join("deno.json").exists() {
        eprintln!(
            "warning: {} is no longer read; q2 now bundles with esbuild. \
             Add an esbuild.json ({{\"alias\": {{...}}}}) if the extension needs import aliases.",
            ext_dir.join("deno.json").display()
        );
    }

    let source = resolve_build_config(
        opts.config.as_deref(),
        ext_esbuild.as_deref(),
        workspace_root.as_deref(),
    );
    let aliases = load_aliases(&source)?;

    let entry_ts = find_entry_ts(&ext_dir)?;
    let output_js = find_output_path(&ext_dir)?;
    let esbuild = resolve_esbuild_binary(&opts, &ext_dir, workspace_root.as_deref())?;

    run_esbuild(&esbuild, &ext_dir, &aliases, &source, &entry_ts, &output_js)?;
    Ok(output_js)
}

// ============================================================================
// Extension directory / entry point helpers
// ============================================================================

/// Resolves `path` to the extension directory.
///
/// - If `path` points directly to `_extension.yml`, use its parent.
/// - If `path` points to a directory, use it directly.
/// - If `path` is `None`, scan the current working directory.
fn resolve_extension_dir(path: Option<&Path>) -> Result<PathBuf> {
    let dir = match path {
        Some(p) if p.file_name().is_some_and(|n| n == "_extension.yml") => p
            .parent()
            .map_or_else(|| PathBuf::from("."), |d| d.to_path_buf()),
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir().context("Failed to get current working directory")?,
    };

    // esbuild runs with `current_dir = dir`, so every path derived from it
    // (entry, output, aliases) must not depend on the caller's cwd.
    let dir = std::path::absolute(&dir)
        .with_context(|| format!("Failed to resolve {}", dir.display()))?;

    // Verify _extension.yml exists in the resolved directory.
    let yml = dir.join("_extension.yml");
    if !yml.exists() {
        bail!(
            "No _extension.yml found in {}. \
             Pass the extension directory or _extension.yml path explicitly.",
            dir.display()
        );
    }

    Ok(dir)
}

/// Finds the TypeScript entry point for the extension.
///
/// Convention: `src/<name>.ts` where `<name>` is the extension directory's
/// last component (the extension name).
fn find_entry_ts(ext_dir: &Path) -> Result<PathBuf> {
    let name = ext_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extension");
    let candidate = ext_dir.join("src").join(format!("{}.ts", name));
    if candidate.exists() {
        return Ok(candidate);
    }
    // Fall back: any .ts in src/.
    let src_dir = ext_dir.join("src");
    if src_dir.is_dir() {
        for entry in std::fs::read_dir(&src_dir)
            .with_context(|| format!("Failed to read {}", src_dir.display()))?
        {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "ts") {
                return Ok(entry.path());
            }
        }
    }
    bail!(
        "No TypeScript entry point found. Expected src/{}.ts inside {}. \
         Create the file or use --config to override the build config.",
        name,
        ext_dir.display()
    )
}

/// Reads the output JS path from the first engine contribution in
/// `_extension.yml`.
fn find_output_path(ext_dir: &Path) -> Result<PathBuf> {
    let yml_path = ext_dir.join("_extension.yml");
    let content = std::fs::read_to_string(&yml_path)
        .with_context(|| format!("Failed to read {}", yml_path.display()))?;

    // Parse with serde_yaml; we only need the `contributes.engines[0].path` field.
    let value: serde_yaml::Value = serde_yaml::from_str(&content)
        .with_context(|| format!("Invalid YAML in {}", yml_path.display()))?;

    let path_str = value
        .get("contributes")
        .and_then(|c| c.get("engines"))
        .and_then(|e| e.as_sequence())
        .and_then(|seq| seq.first())
        .and_then(|entry| entry.get("path"))
        .and_then(|p| p.as_str())
        .with_context(|| {
            format!(
                "Could not find contributes.engines[0].path in {}. \
                 The extension must declare a pre-built .js path.",
                yml_path.display()
            )
        })?;

    Ok(ext_dir.join(path_str))
}

// ============================================================================
// esbuild runner
// ============================================================================

/// Strip the Windows verbatim prefix (`\\?\C:\...`) that
/// `Path::canonicalize` adds. esbuild cannot resolve alias targets that carry
/// it ("Could not resolve ... using the alias feature"). UNC verbatim paths
/// (`\\?\UNC\...`) and every non-verbatim path are returned unchanged.
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// The esbuild arguments for one build. Run with `cwd = ext_dir` and a
/// relative `entry`, because esbuild embeds the entry path in a comment: a
/// fixed relative path keeps the output byte-stable across machines.
fn esbuild_args(
    entry: &Path,
    output: &Path,
    aliases: &BTreeMap<String, PathBuf>,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = [
        "--bundle",
        "--format=esm",
        "--platform=node",
        "--target=node24",
        "--external:node:*",
        "--conditions=source,import",
        "--log-level=warning",
    ]
    .iter()
    .map(Into::into)
    .collect();
    for (name, target) in aliases {
        let mut arg = std::ffi::OsString::from(format!("--alias:{name}="));
        arg.push(strip_verbatim_prefix(target));
        args.push(arg);
    }
    let mut outfile = std::ffi::OsString::from("--outfile=");
    outfile.push(output);
    args.push(outfile);
    args.push(entry.into());
    args
}

/// Turn esbuild's stderr into guidance when it names a known migration problem.
fn explain_failure(stderr: &str, source: &ConfigSource) -> Option<String> {
    if stderr.contains("\"jsr:") || stderr.contains("\"@std/") || stderr.contains("\"npm:") {
        return Some(
            "`jsr:`, `npm:` and `@std/*` imports are Deno-only and are not resolved. \
             Use the `node:` equivalent (node:path, node:fs, node:process, \
             node:child_process, ...) or install the package with npm."
                .to_string(),
        );
    }
    if matches!(source, ConfigSource::Shipped) && stderr.contains("\"@quarto/") {
        return Some(
            "`@quarto/api` and `@quarto/types` are not published yet, so an installed q2 \
             cannot resolve value imports from them. Build inside the q2 workspace \
             (or pass --workspace), or import only types (`import type`), which need no resolution."
                .to_string(),
        );
    }
    None
}

fn run_esbuild(
    esbuild: &Path,
    ext_dir: &Path,
    aliases: &BTreeMap<String, PathBuf>,
    source: &ConfigSource,
    entry: &Path,
    output: &Path,
) -> Result<()> {
    use std::process::Command;

    // Ensure the output parent directory exists.
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    let relative_entry = entry.strip_prefix(ext_dir).unwrap_or(entry);
    let result = Command::new(esbuild)
        .current_dir(ext_dir)
        .args(esbuild_args(relative_entry, output, aliases))
        .output()
        .with_context(|| format!("Failed to run esbuild at {}", esbuild.display()))?;

    // esbuild reports problems on stderr; pass them through either way so
    // warnings are not lost.
    let stderr = String::from_utf8_lossy(&result.stderr);
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    if !result.status.success() {
        let hint = explain_failure(&stderr, source)
            .map(|h| format!("\n{h}"))
            .unwrap_or_default();
        bail!(
            "esbuild exited with status {}. Check the TypeScript source for errors.{hint}",
            result.status
        );
    }

    println!("Built: {} → {}", entry.display(), output.display());
    Ok(())
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    #[test]
    fn strip_verbatim_prefix_handles_windows_forms() {
        let strip = |p: &str| strip_verbatim_prefix(Path::new(p));
        assert_eq!(
            strip(r"\\?\C:\Users\me\q2\src"),
            PathBuf::from(r"C:\Users\me\q2\src")
        );
        assert_eq!(
            strip(r"\\?\UNC\server\share\x"),
            PathBuf::from(r"\\?\UNC\server\share\x")
        );
        assert_eq!(strip("/home/me/q2"), PathBuf::from("/home/me/q2"));
        assert_eq!(strip(r"C:\plain"), PathBuf::from(r"C:\plain"));
    }

    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ---- helpers --------------------------------------------------------

    fn make_dir() -> TempDir {
        tempfile::tempdir().expect("create tempdir")
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, "").unwrap();
    }

    // ---- resolve_build_config tests ------------------------------------

    /// Explicit --config wins over everything else.
    #[test]
    fn explicit_config_wins() {
        let explicit = PathBuf::from("/x/custom.json");
        let source = resolve_build_config(
            Some(&explicit),
            Some(Path::new("/x/ext/esbuild.json")),
            Some(Path::new("/x/workspace")),
        );
        assert_eq!(source, ConfigSource::File(explicit));
    }

    /// Extension-dir esbuild.json wins over workspace and shipped.
    #[test]
    fn ext_dir_esbuild_json_wins_over_workspace() {
        let ext = PathBuf::from("/x/ext/esbuild.json");
        let source = resolve_build_config(None, Some(&ext), Some(Path::new("/x/workspace")));
        assert_eq!(source, ConfigSource::File(ext));
    }

    /// The workspace tier is selected when no explicit/ext config exists.
    #[test]
    fn workspace_selected_when_no_ext_config() {
        let root = PathBuf::from("/x/workspace");
        assert_eq!(
            resolve_build_config(None, None, Some(&root)),
            ConfigSource::Workspace(root)
        );
    }

    /// The shipped template is the final fallback.
    #[test]
    fn shipped_template_is_final_fallback() {
        assert_eq!(
            resolve_build_config(None, None, None),
            ConfigSource::Shipped
        );
    }

    /// `--config` wins with no workspace root (the installed-binary case).
    #[test]
    fn config_wins_without_workspace() {
        let explicit = PathBuf::from("/x/custom.json");
        assert_eq!(
            resolve_build_config(Some(&explicit), None, None),
            ConfigSource::File(explicit)
        );
    }

    // ---- load_aliases tests --------------------------------------------

    #[test]
    fn workspace_aliases_point_at_package_sources() {
        let root = PathBuf::from("/ws");
        let aliases = load_aliases(&ConfigSource::Workspace(root.clone())).unwrap();
        assert_eq!(
            aliases["@quarto/api"],
            root.join("ts-packages/quarto-api/src")
        );
        assert_eq!(
            aliases["@quarto/types"],
            root.join("ts-packages/quarto-types/src")
        );
    }

    #[test]
    fn shipped_template_parses_and_declares_no_aliases() {
        assert!(load_aliases(&ConfigSource::Shipped).unwrap().is_empty());
    }

    #[test]
    fn file_aliases_resolve_relative_to_the_config_dir() {
        let dir = make_dir();
        let config = dir.path().join("esbuild.json");
        fs::write(
            &config,
            r#"{"alias": {"lib": "./vendor/lib.ts", "abs": "/already/absolute"}}"#,
        )
        .unwrap();
        let aliases = load_aliases(&ConfigSource::File(config)).unwrap();
        assert_eq!(aliases["lib"], dir.path().join("./vendor/lib.ts"));
        assert_eq!(aliases["abs"], PathBuf::from("/already/absolute"));
    }

    #[test]
    fn unknown_config_keys_are_rejected() {
        let dir = make_dir();
        let config = dir.path().join("esbuild.json");
        fs::write(&config, r#"{"alais": {}}"#).unwrap();
        let err = load_aliases(&ConfigSource::File(config)).unwrap_err();
        assert!(format!("{err:#}").contains("alais"), "got: {err:#}");
    }

    #[test]
    fn missing_config_file_is_an_error() {
        let dir = make_dir();
        assert!(load_aliases(&ConfigSource::File(dir.path().join("nope.json"))).is_err());
    }

    // ---- esbuild args / failure hints ----------------------------------

    #[test]
    fn esbuild_args_are_fixed_and_carry_aliases() {
        let aliases = BTreeMap::from([("@quarto/api".to_string(), PathBuf::from("/ws/api"))]);
        let args: Vec<String> = esbuild_args(Path::new("src/x.ts"), Path::new("/o/x.js"), &aliases)
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--bundle",
                "--format=esm",
                "--platform=node",
                "--target=node24",
                "--external:node:*",
                "--conditions=source,import",
                "--log-level=warning",
                "--alias:@quarto/api=/ws/api",
                "--outfile=/o/x.js",
                "src/x.ts",
            ]
        );
    }

    #[test]
    fn deno_only_imports_get_a_node_hint() {
        let hint = explain_failure(
            r#"✘ [ERROR] Could not resolve "jsr:@std/path""#,
            &ConfigSource::Shipped,
        )
        .unwrap();
        assert!(hint.contains("node:"), "got: {hint}");
    }

    #[test]
    fn quarto_value_imports_in_the_shipped_tier_name_the_workspace() {
        let stderr = r#"✘ [ERROR] Could not resolve "@quarto/api/claims""#;
        let hint = explain_failure(stderr, &ConfigSource::Shipped).unwrap();
        assert!(hint.contains("--workspace"), "got: {hint}");
        assert!(explain_failure(stderr, &ConfigSource::Workspace(PathBuf::from("/ws"))).is_none());
    }

    // ---- esbuild binary resolution -------------------------------------

    fn options() -> BuildOptions {
        BuildOptions {
            ext_dir: None,
            config: None,
            workspace: false,
            workspace_root: None,
            esbuild_binary: None,
            embedded_esbuild: None,
        }
    }

    #[test]
    fn explicit_esbuild_binary_wins() {
        let mut opts = options();
        opts.esbuild_binary = Some(PathBuf::from("/opt/esbuild"));
        opts.embedded_esbuild = Some(|| panic!("embedded must not be consulted"));
        assert_eq!(
            resolve_esbuild_binary(&opts, Path::new("/nowhere"), None).unwrap(),
            std::path::absolute("/opt/esbuild").unwrap()
        );
    }

    #[test]
    fn relative_binary_paths_are_made_absolute() {
        let mut opts = options();
        opts.esbuild_binary = Some(PathBuf::from("rel/esbuild"));
        let resolved = resolve_esbuild_binary(&opts, Path::new("/nowhere"), None).unwrap();
        assert!(resolved.is_absolute(), "got {}", resolved.display());
    }

    #[test]
    fn placeholder_embed_falls_through_to_node_modules() {
        let Some((pkg, bin)) = esbuild_platform_package() else {
            return;
        };
        let dir = make_dir();
        let binary = dir.path().join("node_modules/@esbuild").join(pkg).join(bin);
        touch(&binary);
        let ext = dir.path().join("a/b/ext");
        fs::create_dir_all(&ext).unwrap();

        let mut opts = options();
        opts.embedded_esbuild = Some(|| Ok(None));
        // ESBUILD_BINARY_PATH is process-global; this test only runs when the
        // harness does not set it (nextest gives each test its own process).
        if std::env::var_os("ESBUILD_BINARY_PATH").is_some() {
            return;
        }
        assert_eq!(resolve_esbuild_binary(&opts, &ext, None).unwrap(), binary);
    }

    #[test]
    fn missing_esbuild_binary_names_both_remedies() {
        if std::env::var_os("ESBUILD_BINARY_PATH").is_some() {
            return;
        }
        let dir = make_dir();
        let err = resolve_esbuild_binary(&options(), dir.path(), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("fetch-esbuild"), "got: {err}");
        assert!(err.contains("npm ci"), "got: {err}");
    }

    // ---- find_workspace_root tests ------------------------------------

    /// Detects workspace root from a nested directory.
    #[test]
    fn find_workspace_root_detects_ts_packages() {
        let dir = make_dir();
        let api_dir = dir.path().join("ts-packages").join("quarto-api");
        fs::create_dir_all(&api_dir).unwrap();

        let nested = dir.path().join("some").join("nested").join("dir");
        fs::create_dir_all(&nested).unwrap();

        let found = find_workspace_root(&nested);
        assert_eq!(found, Some(dir.path().to_path_buf()));
    }

    /// Returns None when ts-packages/quarto-api is absent.
    #[test]
    fn find_workspace_root_returns_none_when_absent() {
        let dir = make_dir();
        let nested = dir.path().join("deep").join("dir");
        fs::create_dir_all(&nested).unwrap();

        assert!(find_workspace_root(&nested).is_none());
    }

    // ---- resolve_extension_dir tests ----------------------------------

    #[test]
    fn resolve_extension_dir_from_directory() {
        let dir = make_dir();
        touch(&dir.path().join("_extension.yml"));

        let resolved = resolve_extension_dir(Some(dir.path())).unwrap();
        assert_eq!(resolved, std::path::absolute(dir.path()).unwrap());
    }

    #[test]
    fn resolve_extension_dir_from_yml_path() {
        let dir = make_dir();
        let yml = dir.path().join("_extension.yml");
        touch(&yml);

        let resolved = resolve_extension_dir(Some(&yml)).unwrap();
        assert_eq!(resolved, std::path::absolute(dir.path()).unwrap());

        // A relative path resolves to an absolute directory.
        let relative = resolve_extension_dir(Some(Path::new("."))).unwrap_err();
        assert!(
            relative.to_string().contains("No _extension.yml"),
            "got {relative}"
        );
    }

    #[test]
    fn resolve_extension_dir_fails_without_yml() {
        let dir = make_dir();
        // No _extension.yml created.
        let result = resolve_extension_dir(Some(dir.path()));
        assert!(result.is_err(), "should fail when _extension.yml is absent");
    }
}
