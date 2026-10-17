/**
 * Q2-M1: project-mode transitive include invalidation, real Chromium and
 * real q2 preview binary. The pre-M1 /api/preview/deps endpoint returned
 * only outer.qmd; a change to leaf.qmd was dropped as unrelated.
 *
 * Neither this spec nor its CI job mocks WASM, React, or WebSocket
 * events. Visible iframe content and __renderTicks are the oracle.
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
  await page.goto(server.url);
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  await page.waitForTimeout(200); // commit the initial deps result to SPA state

  const baseline = await ticks(page);
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), LEAF_EDITED);
  await waitForInnerText(page, 'LEAF-SENTINEL-EDITED');
  expect(await hasInnerText(page, 'LEAF-SENTINEL-INITIAL')).toBe(false);
  expect(await ticks(page)).toBeGreaterThan(baseline);
  await page.waitForTimeout(200);

  const settledTicks = await ticks(page);
  await writeFile(path.join(server.projectDir, 'sibling.qmd'), '# Changed but unrelated\n');
  await page.waitForTimeout(2500);
  expect(await ticks(page)).toBe(settledTicks);
});

test('removing and reintroducing a nested include updates selective invalidation', async ({ page }) => {
  await page.goto(server.url);
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  await page.waitForTimeout(200);

  await writeFile(path.join(server.projectDir, 'outer.qmd'), '## Outer content\n\nLeaf removed\n');
  await waitForInnerText(page, 'Leaf removed');
  expect(await hasInnerText(page, 'LEAF-SENTINEL-INITIAL')).toBe(false);
  await page.waitForTimeout(200);

  const settledTicks = await ticks(page);
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), 'LEAF-SENTINEL-INERT\n');
  await page.waitForTimeout(2500);
  expect(await ticks(page)).toBe(settledTicks);

  await writeFile(path.join(server.projectDir, 'outer.qmd'), OUTER);
  await waitForInnerText(page, 'LEAF-SENTINEL-INERT');
});

test('changed sidebar member re-renders active page through core document edges', async ({ page }) => {
  // A separate server fixture is needed because this test exercises profile
  // dependencies rather than include files. Shut down the beforeEach server.
  await server.stop();
  server = await startPreviewServer({
    fixtureFiles: [
      { path: '_quarto.yml', content: 'project:\n  type: website\nwebsite:\n  sidebar:\n    contents: [index.qmd, about.qmd]\n' },
      { path: 'index.qmd', content: '---\ntitle: Home\n---\n\n# Home content\n' },
      { path: 'about.qmd', content: '---\ntitle: About original\n---\n\n# About\n' },
      { path: 'unrelated.md', content: '# Unrelated note\n' },
    ],
  });
  await page.goto(server.url);
  await waitForInnerText(page, 'Home content');
  await page.waitForTimeout(300);
  const before = await ticks(page);
  await writeFile(path.join(server.projectDir, 'about.qmd'), '---\ntitle: About revised\n---\n\n# About\n');
  await page.waitForFunction(
    (oldTicks) => ((window as unknown as { __renderTicks?: number }).__renderTicks ?? 0) > oldTicks,
    before,
    { timeout: 15000 },
  );
  expect(await ticks(page)).toBeGreaterThan(before);
});

test('single-file preview tracks nested included source changes', async ({ page }) => {
  await server.stop();
  server = await startPreviewServer({
    targetFile: 'index.qmd',
    fixtureFiles: [
      { path: 'index.qmd', content: INDEX },
      { path: 'outer.qmd', content: OUTER },
      { path: 'leaf.qmd', content: LEAF_INITIAL },
      { path: 'sibling.qmd', content: SIBLING },
    ],
  });
  await page.goto(server.url);
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  await page.waitForTimeout(200);

  const before = await ticks(page);
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), LEAF_EDITED);
  await waitForInnerText(page, 'LEAF-SENTINEL-EDITED');
  expect(await ticks(page)).toBeGreaterThan(before);
  await page.waitForTimeout(200);

  const settled = await ticks(page);
  await writeFile(path.join(server.projectDir, 'sibling.qmd'), '# Still unrelated\n');
  await page.waitForTimeout(2500);
  expect(await ticks(page)).toBe(settled);
});
