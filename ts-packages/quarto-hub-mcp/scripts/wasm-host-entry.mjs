// Entry point for the prebundled WASM host (dist/wasm-host.mjs).
//
// `build-wasm-host.mjs` bundles this with esbuild, aliasing the
// wasm-bindgen JS's Vite-root-absolute bridge imports
// (/src/wasm-js-bridge/*) to the ts-packages/wasm-js-bridge sources,
// so the same wasm-quarto-hub-client module the browser preview runs
// loads in plain Node. dart-sass is bundled INTO the host (aliased to
// its pure-JS default entry — the `node` condition pulls in
// sass.node.js, which dies on `require("util")` inside the ESM
// bundle); the html render compiles themes even for a
// diagnostics-only run, and the embedded dist-bundle has no
// node_modules to resolve `sass` from at runtime.
import { readFile } from 'node:fs/promises';
import init, { vfs_list_files, vfs_read_file } from 'wasm-quarto-hub-client';
import { setVfsCallbacks } from '/src/wasm-js-bridge/sass.js';

export * from 'wasm-quarto-hub-client';

let ready;

/**
 * Wire the sass bridge's VFS access, mirroring preview-runtime's
 * wasmRenderer: the bridge's importer resolves @import/@use through
 * these callbacks, so without them every theme compilation fails with
 * "Can't find stylesheet to import". The bridge module instance is
 * shared with the wasm-bindgen glue (same bundled path), so the Rust
 * side sees the callbacks too.
 */
function wireSassVfsCallbacks() {
  const read = (path) => {
    try {
      const result = JSON.parse(vfs_read_file(path));
      return result.success && result.content !== undefined ? result.content : null;
    } catch {
      return null;
    }
  };
  setVfsCallbacks(read, (path) => read(path) !== null, () => {
    try {
      const result = JSON.parse(vfs_list_files());
      return result.success && result.files ? result.files : [];
    } catch {
      return [];
    }
  });
}

/**
 * Initialize the WASM module from the binary shipped next to this
 * file. Idempotent; callers await it before any render/vfs call.
 */
export function ensureInit() {
  ready ??= readFile(new URL('./wasm_quarto_hub_client_bg.wasm', import.meta.url))
    .then((bytes) => init({ module_or_path: bytes }))
    .then(() => wireSassVfsCallbacks());
  return ready;
}
