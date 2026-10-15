/**
 * Q2-M1: project-mode transitive include invalidation, real Chromium and
 * real q2 preview binary. The pre-M1 /api/preview/deps endpoint returned
 * only outer.qmd; a change to leaf.qmd was dropped as unrelated.
 *
 * Neither this spec nor its CI job uses mocked WASM, React, or websocket
 * events. The rendered iframe text and __renderTicks are the oracle.
 */
import { test, expect, type Page } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const INDEX = '# Root document\n\n{{< include outer.qmd >}}\n';
const OUTER = '## Outer content\n\n{{< include leaf.qmd >}}\n';
const LEAF_INITIAL = 'LEAF-SENTINEL-INITIAL\n';
const LEAF_EDITED = 'LEAF-SENTINEL-EDITED\n';
const SIBLING = '# Unrelated sibling\n';

async function waitForInnerText(page: Page, needle: string): Promise<void> {
  await page.waitForFunction(
    (text) => {
      const frame = document.querySelector('iframe');
      return frame?.contentDocument?.body?.textContent?.includes(text) === true;
    },
    needle,
    { timeout: 30_000 },
  );
}

async function hasInnerText(page: Page, needle: string): Promise<boolean> {
  return page.evaluate((text) => {
    const frame = document.querySelector('iframe');
    return frame?.contentDocument?.body?.textContent?.includes(text) === true;
  }, needle);
}

async function ticks(page: Page): Promise<number> {
  return page.evaluate(() =>
    (window as unknown as { __renderTicks?: number }).__renderTicks ?? 0,
  );
}

function depsResponse(page: Page) {
  return page.waitForResponse(
    (r) => r.url().includes('/api/preview/deps?page=index.qmd') && r.status() === 200,
    { timeout: 20_000 },
  );
}

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [
      { path: '_quarto.yml', content: 'project:\n  type: website\n' },
      { path: 'index.qmd', content: INDEX },
      { path: 'outer.qmd', content: OUTER },
      { path: 'leaf.qmd', content: LEAF_INITIAL },
      { path: 'sibling.qmd', content: SIBLING },
    ],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('nested leaf edit refreshes the active page; unrelated sibling edit does not', async ({ page }) => {
  const firstDeps = depsResponse(page);
  await page.goto(server.url);
  expect((await (await firstDeps).json()).deps).toEqual(['leaf.qmd', 'outer.qmd']);
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  await page.waitForTimeout(200); // commit the initial deps result to SPA state

  const nextDeps = depsResponse(page);
  const baseline = await ticks(page);
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), LEAF_EDITED);
  await waitForInnerText(page, 'LEAF-SENTINEL-EDITED');
  expect(await hasInnerText(page, 'LEAF-SENTINEL-INITIAL')).toBe(false);
  expect(await ticks(page)).toBeGreaterThan(baseline);
  expect((await (await nextDeps).json()).deps).toEqual(['leaf.qmd', 'outer.qmd']);
  await page.waitForTimeout(200);

  const settledTicks = await ticks(page);
  await writeFile(path.join(server.projectDir, 'sibling.qmd'), '# Changed but unrelated\n');
  await page.waitForTimeout(2500);
  expect(await ticks(page)).toBe(settledTicks);
});

test('removing and reintroducing a nested include updates selective invalidation', async ({ page }) => {
  const firstDeps = depsResponse(page);
  await page.goto(server.url);
  await firstDeps;
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  await page.waitForTimeout(200);

  const withoutLeaf = depsResponse(page);
  await writeFile(path.join(server.projectDir, 'outer.qmd'), '## Outer content\n\nLeaf removed\n');
  await waitForInnerText(page, 'Leaf removed');
  expect((await (await withoutLeaf).json()).deps).toEqual(['outer.qmd']);
  expect(await hasInnerText(page, 'LEAF-SENTINEL-INITIAL')).toBe(false);
  await page.waitForTimeout(200);

  const settledTicks = await ticks(page);
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), 'LEAF-SENTINEL-INERT\n');
  await page.waitForTimeout(2500);
  expect(await ticks(page)).toBe(settledTicks);

  const withLeafAgain = depsResponse(page);
  await writeFile(path.join(server.projectDir, 'outer.qmd'), OUTER);
  await waitForInnerText(page, 'LEAF-SENTINEL-INERT');
  expect((await (await withLeafAgain).json()).deps).toEqual(['leaf.qmd', 'outer.qmd']);
});
