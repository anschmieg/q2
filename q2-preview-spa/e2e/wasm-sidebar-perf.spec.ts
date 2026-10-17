/**
 * Measure the actual optimized browser-WASM ProjectDependencyGraph query.
 *
 * The CI job builds wasm-quarto-hub-client in --release mode and runs wasm-opt
 * -Oz with the optional m1-benchmark export enabled. The benchmark module is
 * NOT compiled in normal production builds. Test page imports the actual
 * generated wasm-bindgen ESM and initializes the optimized binary.
 *
 * Synthetic Pass-1 profiles isolate dependency querying from unrelated WASM
 * project boot, VFS hydration, native server watching and full renders.
 */
import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import type { AddressInfo } from 'node:net';

test('optimized browser WASM sidebar query scales without clique materialization', async ({ page }) => {
  test.skip(process.env.Q2_M1_BENCH !== '1', 'requires optimized CI WASM benchmark export');

  // Playwright may transpile ESM tests into a cache directory; unlike
  // import.meta.dirname, cwd is the configured q2-preview-spa project root.
  // Validate both artifacts up front rather than masking an ENOENT as a
  // generic browser-side "Failed to fetch dynamically imported module".
  const pkgDir = path.resolve(process.cwd(), '..', 'crates', 'wasm-quarto-hub-client', 'pkg');
  const jsName = 'wasm_quarto_hub_client.js';
  const wasmName = 'wasm_quarto_hub_client_bg.wasm';
  const [jsSource, wasmStat] = await Promise.all([
    readFile(path.join(pkgDir, jsName)),
    stat(path.join(pkgDir, wasmName)),
  ]);
  expect(jsSource.length).toBeGreaterThan(0);
  expect(wasmStat.size).toBeGreaterThan(0);
  const server = createServer(async (request, response) => {
    const file = new URL(request.url ?? '/', 'http://localhost').pathname;
    if (file === '/') {
      response.setHeader('Content-Type', 'text/html');
      response.end('<!doctype html><title>Optimized WASM M1 benchmark</title>');
      return;
    }
    const allowed = new Map([
      ['/wasm_quarto_hub_client.js', 'wasm_quarto_hub_client.js'],
      ['/wasm_quarto_hub_client_bg.wasm', 'wasm_quarto_hub_client_bg.wasm'],
    ]);
    const name = allowed.get(file);
    if (!name) {
      response.writeHead(404).end();
      return;
    }
    try {
      const bytes = await readFile(path.join(pkgDir, name));
      response.setHeader('Content-Type', name.endsWith('.wasm') ? 'application/wasm' : 'text/javascript');
      response.end(bytes);
    } catch (error) {
      console.error('Optimized WASM benchmark fixture read failed', { pkgDir, file, error });
      response.writeHead(500, { 'Content-Type': 'text/plain' }).end(String(error));
    }
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address() as AddressInfo;
  try {
    await page.goto(`http://127.0.0.1:${address.port}/`);
    // Make module/wasm serving a separate and actionable assertion.
    const [moduleResponse, wasmResponse] = await Promise.all([
      page.request.get(`http://127.0.0.1:${address.port}/${jsName}`),
      page.request.get(`http://127.0.0.1:${address.port}/${wasmName}`),
    ]);
    expect(moduleResponse.status()).toBe(200);
    expect(wasmResponse.status()).toBe(200);
    expect(moduleResponse.headers()['content-type']).toContain('javascript');
    expect(wasmResponse.headers()['content-type']).toContain('wasm');
    const rows = await page.evaluate(async () => {
      // This is loaded from the produced (wasm-opt -Oz) pkg, not mocks.
      const wasm = await import('/wasm_quarto_hub_client.js');
      await wasm.default('/wasm_quarto_hub_client_bg.wasm');
      return [128, 512, 1000].map((pages) =>
        JSON.parse(wasm.benchmark_live_sidebar_query(pages, 21)) as {
          pages: number;
          inputs: number;
          iterations: number;
          optimized_wasm_ms_p50: number;
        },
      );
    });
    for (const row of rows) {
      expect(row.inputs).toBe(row.pages);
      expect(row.iterations).toBe(21);
      expect(Number.isFinite(row.optimized_wasm_ms_p50)).toBe(true);
      expect(row.optimized_wasm_ms_p50).toBeGreaterThanOrEqual(0);
      console.log(`q2_m1_optimized_wasm_perf ${JSON.stringify(row)}`);
    }
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => error ? reject(error) : resolve()),
    );
  }
});
