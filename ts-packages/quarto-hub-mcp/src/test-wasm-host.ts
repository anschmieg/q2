/**
 * Test helper: is the real prebundled WASM render host available?
 *
 * `npm run build` writes a STUB `dist/wasm-host.mjs` (plus a
 * `wasm-host.mjs.stub` marker) when the Rust WASM artifacts are absent
 * — the situation in the no-Rust/no-WASM CI leg (workspace-ts-suites,
 * GH #250). Suites that need the real render pipeline skip themselves
 * in that leg; they still run wherever the WASM was built.
 */

import { existsSync } from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const pkgRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(pkgRoot, '../..');

/** `dist/wasm-host.mjs` exists and is the real prebundled host, not the stub. */
export function realWasmHostAvailable(): boolean {
  const host = path.join(pkgRoot, 'dist', 'wasm-host.mjs');
  return existsSync(host) && !existsSync(`${host}.stub`);
}

/** The Rust WASM build products (`crates/wasm-quarto-hub-client/pkg/`) exist. */
export function wasmPkgAvailable(): boolean {
  return existsSync(
    path.join(repoRoot, 'crates', 'wasm-quarto-hub-client', 'pkg', 'wasm_quarto_hub_client_bg.wasm'),
  );
}
