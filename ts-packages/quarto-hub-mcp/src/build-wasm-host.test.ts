/**
 * Tests for scripts/build-wasm-host.mjs (driven through `node`, same
 * pattern as bundle.test.ts).
 *
 * The script bundles the Rust WASM render host into dist/. CI legs
 * without the Rust toolchain (workspace-ts-suites, GH #250) do not
 * have crates/wasm-quarto-hub-client/pkg/, so the package build must
 * degrade to a stub host instead of failing — while the release
 * bundle path must keep failing loudly.
 */

import { describe, it, expect } from 'vitest';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const pkgRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const script = path.join(pkgRoot, 'scripts', 'build-wasm-host.mjs');

function makeDirs(): { out: string; emptyPkg: string } {
  const out = mkdtempSync(path.join(os.tmpdir(), 'wasm-host-build-'));
  return { out, emptyPkg: path.join(out, 'no-such-pkg') };
}

describe('build-wasm-host without the Rust WASM artifacts', () => {
  it('--allow-stub writes a stub host whose ensureInit fails clearly', async () => {
    const { out, emptyPkg } = makeDirs();
    try {
      execFileSync('node', [script, '--allow-stub', '--out-dir', out, '--pkg-dir', emptyPkg], {
        stdio: 'pipe',
      });
      const hostPath = path.join(out, 'wasm-host.mjs');
      expect(existsSync(hostPath)).toBe(true);
      // The marker lets tests distinguish the stub from the real host.
      expect(existsSync(`${hostPath}.stub`)).toBe(true);
      const host = (await import(pathToFileURL(hostPath).href)) as {
        ensureInit(): Promise<unknown>;
      };
      await expect(host.ensureInit()).rejects.toThrow(/build:wasm/);
    } finally {
      rmSync(out, { recursive: true, force: true });
    }
  });

  it('without --allow-stub the build refuses (release bundle path)', () => {
    const { out, emptyPkg } = makeDirs();
    try {
      const res = spawnSync('node', [script, '--out-dir', out, '--pkg-dir', emptyPkg], {
        stdio: 'pipe',
      });
      expect(res.status).not.toBe(0);
      expect(res.stderr.toString()).toMatch(/build:wasm/);
    } finally {
      rmSync(out, { recursive: true, force: true });
    }
  });
});
