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
    // wasm-bindgen web targets may emit ESM snippet imports alongside the
    // top-level glue and binary. Restrict serving to generated JS/WASM under
    // pkg/, but do not assume the module is self-contained.
    let relative: string;
    try {
      relative = decodeURIComponent(file.slice(1));
    } catch {
      response.writeHead(400).end();
      return;
    }
    if (!relative || relative.split('/').some((part) => !part || part === '.' || part === '..')
        || !/\.(?:js|mjs|wasm)$/.test(relative)) {
      response.writeHead(404).end();
      return;
    }
    const absolute = path.resolve(pkgDir, relative);
    if (path.relative(pkgDir, absolute).startsWith('..')) {
      response.writeHead(403).end();
      return;
    }
    try {
      const bytes = await readFile(absolute);
      response.setHeader('Content-Type', relative.endsWith('.wasm') ? 'application/wasm' : 'text/javascript');
      response.end(bytes);
    } catch (error) {
      console.error('Optimized WASM fixture asset unavailable', { relative, absolute, error });
      response.writeHead(404, { 'Content-Type': 'text/plain' }).end(String(error));
    }
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address() as AddressInfo;
  try {
    const browserErrors: string[] = [];
    page.on('console', (message) => {
      if (message.type() === 'error') browserErrors.push(message.text());
    });
    page.on('pageerror', (error) => browserErrors.push(error.message));
    page.on('response', (response) => {
      if (!response.ok()) browserErrors.push(`HTTP ${response.status()} ${response.url()}`);
    });
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
    let rows: Array<{
      pages: number;
      inputs: number;
      iterations: number;
      optimized_wasm_ms_p50: number;
    }>;
    try {
      rows = await page.evaluate(async () => {
        // Actual wasm-bindgen ESM with the wasm-opt -Oz binary; no mocks.
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
    } catch (error) {
      // Include browser network/MIME/ESM diagnostics; a generic dynamic
      // import TypeError gives almost no actionable information in CI.
      throw new Error(`Optimized WASM import/benchmark failed: ${String(error)}. Browser errors: ${browserErrors.join(' | ') || '(none)'}`);
    }
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
