// Prebundle the WASM host for plain-Node consumers (dist/ and the
// esbuild dist-bundle). Produces:
//   dist/wasm-host.mjs                    — bundled wasm-bindgen JS + bridges
//   dist/wasm_quarto_hub_client_bg.wasm   — the WASM binary, loaded by the host
//
// The wasm-bindgen JS imports its bridge modules by the Vite-root
// paths hub-client serves them from; the alias plugin maps those to
// the ts-packages/wasm-js-bridge sources.
//
// When the Rust WASM artifacts (crates/wasm-quarto-hub-client/pkg/) are
// absent — fresh clones and the no-Rust/no-WASM CI legs
// (workspace-ts-suites, GH #250) — the package `build` script passes
// --allow-stub and gets a stub host whose ensureInit() throws a clear
// error, so the build and the non-WASM tests still pass and get_errors
// degrades cleanly. The release bundle (bundle.mjs) does NOT allow the
// stub: a stub must never ship to users.
import * as esbuild from 'esbuild';
import { copyFile, mkdir, rm, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const pkgDir = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const repoRoot = path.resolve(pkgDir, '../..');
// The wasm-pack output directory itself. hub-client reaches it through
// the committed hub-client/wasm-quarto-hub-client symlink, but that
// symlink is a plain text file on a Windows checkout (no core.symlinks)
// and dangles anywhere WASM has not been built, so resolve the real
// path: it is what the release pipeline's web-payloads artifact
// extracts into on every build leg.
const defaultWasmPkg = path.join(repoRoot, 'crates/wasm-quarto-hub-client/pkg');
const bridgeDir = path.join(repoRoot, 'ts-packages/wasm-js-bridge/src');

/** Locate node_modules/sass/sass.default.js (workspace-hoisted or local). */
function resolveSassDefault() {
  for (const base of [repoRoot, pkgDir, path.join(bridgeDir, '..')]) {
    const p = path.join(base, 'node_modules', 'sass', 'sass.default.js');
    if (existsSync(p)) return p;
  }
  throw new Error('could not locate node_modules/sass/sass.default.js');
}

const STUB_SOURCE = `// STUB — written by build-wasm-host.mjs because the Rust WASM
// artifacts (crates/wasm-quarto-hub-client/pkg/) were absent at build
// time. The wasm-host.mjs.stub marker next to this file lets the test
// suites skip themselves. Rebuild after \`npm run build:wasm\`
// (hub-client) to get the real host.
export async function ensureInit() {
  throw new Error(
    'Quarto WASM render host not available: this build lacks the Rust WASM artifacts. ' +
      'Run \`npm run build:wasm\` (hub-client) and rebuild quarto-hub-mcp.',
  );
}
`;

/**
 * Build the host bundle + WASM binary into `opts.outDir`.
 *
 * opts.allowStub — write a stub host when the Rust WASM artifacts are
 *   missing, instead of throwing (dev builds / no-WASM CI legs).
 * opts.wasmPkgDir — wasm-pack output directory; defaults to
 *   crates/wasm-quarto-hub-client/pkg (overridable for tests).
 */
export async function buildWasmHost({ outDir, allowStub = false, wasmPkgDir = defaultWasmPkg }) {
  await mkdir(outDir, { recursive: true });
  const wasmJs = path.join(wasmPkgDir, 'wasm_quarto_hub_client.js');
  const wasmBin = path.join(wasmPkgDir, 'wasm_quarto_hub_client_bg.wasm');
  const marker = path.join(outDir, 'wasm-host.mjs.stub');

  if (!existsSync(wasmJs) || !existsSync(wasmBin)) {
    if (!allowStub) {
      throw new Error(
        `Rust WASM artifacts not found in ${wasmPkgDir}. ` +
          'Run `npm run build:wasm` (hub-client) first.',
      );
    }
    await writeFile(path.join(outDir, 'wasm-host.mjs'), STUB_SOURCE);
    await writeFile(marker, 'stub: built without the Rust WASM artifacts\n');
    console.log(
      `wasm-host STUB bundled into ${path.relative(pkgDir, outDir)}/ (no Rust WASM artifacts)`,
    );
    return;
  }

  const bridgeAlias = {
    name: 'wasm-bridge-alias',
    setup(build) {
      build.onResolve({ filter: /^\/src\/wasm-js-bridge\// }, (args) => ({
        path: path.join(bridgeDir, path.basename(args.path)),
      }));
      build.onResolve({ filter: /^wasm-quarto-hub-client$/ }, () => ({
        path: wasmJs,
      }));
      // dart-sass's `node` export condition resolves to sass.node.js,
      // which require()s Node builtins ("Dynamic require of util is
      // not supported" inside the ESM bundle). Force the pure-JS
      // default entry — the same one hub-client resolves to in the
      // browser. (The package's closed `exports` map blocks subpath
      // resolution, so probe node_modules directly.)
      build.onResolve({ filter: /^sass$/ }, () => ({ path: resolveSassDefault() }));
    },
  };

  await esbuild.build({
    entryPoints: [path.join(pkgDir, 'scripts/wasm-host-entry.mjs')],
    bundle: true,
    platform: 'node',
    format: 'esm',
    outfile: path.join(outDir, 'wasm-host.mjs'),
    plugins: [bridgeAlias],
    banner: {
      // Bundled CJS deps (dart-sass's dart2js runtime) detect Node and
      // call require("url") & friends; esbuild's ESM-output __require
      // shim throws for those unless a real require is in scope, so
      // give the bundle one (vitest's runner masks this — plain Node
      // hits it on every theme compile).
      js: "import { createRequire as __bannerCreateRequire } from 'node:module'; const require = __bannerCreateRequire(import.meta.url);",
    },
    // dart-sass is pure JS and the html render's theme compilation
    // needs it, so it rides inside the host bundle (the embedded
    // dist-bundle has no node_modules to resolve it from at runtime).
    logLevel: 'warning',
  });
  await copyFile(wasmBin, path.join(outDir, 'wasm_quarto_hub_client_bg.wasm'));
  // A real host replaces any stub left by an earlier no-WASM build.
  await rm(marker, { force: true });
  console.log(`wasm-host bundled into ${path.relative(pkgDir, outDir)}/`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const args = process.argv.slice(2);
  const valueOf = (name) => {
    const i = args.indexOf(name);
    return i >= 0 ? args[i + 1] : undefined;
  };
  await buildWasmHost({
    outDir: valueOf('--out-dir') ?? path.join(pkgDir, 'dist'),
    allowStub: args.includes('--allow-stub'),
    wasmPkgDir: valueOf('--pkg-dir') ?? defaultWasmPkg,
  });
}
