/**
 * Q2-M1: project-mode transitive include invalidation, real Chromium and
 * real q2 preview binary. The pre-M1 /api/preview/deps endpoint returned
 * only outer.qmd; a change to leaf.qmd was dropped as unrelated.
 *
 * Neither this spec nor its CI job mocks WASM, React, or WebSocket
 * events. Visible iframe content and __renderTicks are the oracle.
 */
import { test, expect, type Page } from '@playwright/test';
import { writeFile, unlink } from 'node:fs/promises';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
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

function observedUrl(url: string): string {
  const target = new URL(url);
  target.searchParams.set('__q2_test_sync', '1');
  return target.toString();
}

async function observedUpdates(page: Page, file: string): Promise<number> {
  return page.evaluate((name) =>
    (window as unknown as { __q2SyncEvents?: Record<string, number> })
      .__q2SyncEvents?.[name] ?? 0,
  file);
}

async function waitForObservedUpdate(page: Page, file: string, prior: number): Promise<void> {
  await page.waitForFunction(
    ({ name, previous }) => (
      (window as unknown as { __q2SyncEvents?: Record<string, number> })
        .__q2SyncEvents?.[name] ?? 0
    ) > previous,
    { name: file, previous: prior },
    { timeout: 15_000 },
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
  await page.goto(observedUrl(server.url));
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  await page.waitForTimeout(200); // commit the initial deps result to SPA state

  const baseline = await ticks(page);
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), LEAF_EDITED);
  await waitForInnerText(page, 'LEAF-SENTINEL-EDITED');
  expect(await hasInnerText(page, 'LEAF-SENTINEL-INITIAL')).toBe(false);
  expect(await ticks(page)).toBeGreaterThan(baseline);
  await page.waitForTimeout(200);

  const settledTicks = await ticks(page);
  const observedBefore = await observedUpdates(page, 'sibling.qmd');
  await writeFile(path.join(server.projectDir, 'sibling.qmd'), '# Changed but unrelated\n');
  await waitForObservedUpdate(page, 'sibling.qmd', observedBefore);
  await page.waitForTimeout(800);
  expect(await ticks(page)).toBe(settledTicks);
});

test('removing and reintroducing a nested include updates selective invalidation', async ({ page }) => {
  await page.goto(observedUrl(server.url));
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  await page.waitForTimeout(200);

  await writeFile(path.join(server.projectDir, 'outer.qmd'), '## Outer content\n\nLeaf removed\n');
  await waitForInnerText(page, 'Leaf removed');
  expect(await hasInnerText(page, 'LEAF-SENTINEL-INITIAL')).toBe(false);
  await page.waitForTimeout(200);

  const settledTicks = await ticks(page);
  const observedBefore = await observedUpdates(page, 'leaf.qmd');
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), 'LEAF-SENTINEL-INERT\n');
  await waitForObservedUpdate(page, 'leaf.qmd', observedBefore);
  await page.waitForTimeout(800);
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
  await page.goto(observedUrl(server.url));
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
  await page.goto(observedUrl(server.url));
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  await page.waitForTimeout(200);

  const before = await ticks(page);
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), LEAF_EDITED);
  await waitForInnerText(page, 'LEAF-SENTINEL-EDITED');
  expect(await ticks(page)).toBeGreaterThan(before);
  await page.waitForTimeout(200);

  // In single-file mode the native watcher intentionally excludes siblings.
  // The project-mode test above, not this case, tests *delivered* negatives.
  const observedBefore = await observedUpdates(page, 'leaf.qmd');
  await writeFile(path.join(server.projectDir, 'leaf.qmd'), 'LEAF-SENTINEL-SINGLE-FILE-SECOND\n');
  await waitForObservedUpdate(page, 'leaf.qmd', observedBefore);
  await waitForInnerText(page, 'LEAF-SENTINEL-SINGLE-FILE-SECOND');
});

test('records informational cold-load and warm-edit latency for project preview', async ({ page }) => {
  // End-to-end sample includes sync, WASM, React, and iframe paint.
  // Timing is informational: CI machines differ and elapsed-time
  // thresholds are not regression assertions.
  const coldStart = performance.now();
  await page.goto(observedUrl(server.url));
  await waitForInnerText(page, 'LEAF-SENTINEL-INITIAL');
  const coldLoadMs = Math.round(performance.now() - coldStart);
  const editMs: number[] = [];
  for (let i = 0; i < 3; i++) {
    const sentinel = `LEAF-SENTINEL-PERF-${i}`;
    const start = performance.now();
    await writeFile(path.join(server.projectDir, 'leaf.qmd'), `${sentinel}\n`);
    await waitForInnerText(page, sentinel);
    editMs.push(Math.round(performance.now() - start));
  }
  editMs.sort((a, b) => a - b);
  console.log(
    `q2_m1_browser_perf scenario=project_nested_include cold_load_ms=${coldLoadMs} warm_edit_ms_p50=${editMs[1]} warm_edit_ms_range=${editMs[0]}..${editMs[2]}`,
  );
});

// Site-root hrefs with URL fragments must map to the same sync identity
// that the sidebar actually renders, rather than a literal "about.qmd#hash".
test('site-root sidebar href with fragment invalidates on target edit', async ({ page }) => {
  await server.stop();
  server = await startPreviewServer({
    fixtureFiles: [
      { path: '_quarto.yml', content: 'project:\n  type: website\nwebsite:\n  sidebar:\n    contents: [index.qmd, "/about.qmd#intro"]\n' },
      { path: 'index.qmd', content: '# Home sentinel\n' },
      { path: 'about.qmd', content: '---\ntitle: Before sidebar target\n---\n\n# Intro\n' },
    ],
  });
  await page.goto(observedUrl(server.url));
  await waitForInnerText(page, 'Home sentinel');
  await page.waitForTimeout(300);
  const beforeTicks = await ticks(page);
  const beforeEvents = await observedUpdates(page, 'about.qmd');
  await writeFile(path.join(server.projectDir, 'about.qmd'), '---\ntitle: After sidebar target\n---\n\n# Intro\n');
  await waitForObservedUpdate(page, 'about.qmd', beforeEvents);
  await page.waitForFunction(
    (count) => ((window as unknown as { __renderTicks?: number }).__renderTicks ?? 0) > count,
    beforeTicks,
    { timeout: 15_000 },
  );
});

test('frontmatter sidebar href resolves against its declaring document', async ({ page }) => {
  await server.stop();
  server = await startPreviewServer({
    fixtureFiles: [
      { path: '_quarto.yml', content: 'project:\n  type: website\n' },
      {
        path: 'docs/index.qmd',
        content: '---\nwebsite:\n  sidebar:\n    contents: [index.qmd, "about.qmd#intro"]\n---\n\n# Nested sidebar home\n',
      },
      { path: 'docs/about.qmd', content: '---\ntitle: Before nested target\n---\n\n# Intro\n' },
      { path: 'other.qmd', content: '# Other\n' },
    ],
  });
  const target = new URL(observedUrl(server.url));
  target.searchParams.set('page', 'docs/index.qmd');
  await page.goto(target.toString());
  await waitForInnerText(page, 'Nested sidebar home');
  await page.waitForTimeout(300);
  const baseline = await ticks(page);
  const observedBefore = await observedUpdates(page, 'docs/about.qmd');
  await writeFile(path.join(server.projectDir, 'docs/about.qmd'), '---\ntitle: Updated nested target\n---\n\n# Intro\n');
  await waitForObservedUpdate(page, 'docs/about.qmd', observedBefore);
  await page.waitForFunction(
    (count) => ((window as unknown as { __renderTicks?: number }).__renderTicks ?? 0) > count,
    baseline,
    { timeout: 15_000 },
  );
});

test('single-file preview enrolls an include added after boot and follows later edits', async ({ page }) => {
  await server.stop();
  server = await startPreviewServer({
    targetFile: 'index.qmd',
    fixtureFiles: [
      { path: 'index.qmd', content: '# Dynamic include home\n' },
      { path: 'added.qmd', content: 'DYNAMIC-ADDED-INITIAL\n' },
      { path: 'unrelated.qmd', content: '# Unrelated sibling\n' },
    ],
  });
  await page.goto(observedUrl(server.url));
  await waitForInnerText(page, 'Dynamic include home');
  await writeFile(
    path.join(server.projectDir, 'index.qmd'),
    '# Dynamic include home\n\n{{< include added.qmd >}}\n',
  );
  await waitForInnerText(page, 'DYNAMIC-ADDED-INITIAL');

  const observed = await observedUpdates(page, 'added.qmd');
  await writeFile(path.join(server.projectDir, 'added.qmd'), 'DYNAMIC-ADDED-EDITED\n');
  await waitForObservedUpdate(page, 'added.qmd', observed);
  await waitForInnerText(page, 'DYNAMIC-ADDED-EDITED');

  const unrelatedBefore = await observedUpdates(page, 'unrelated.qmd');
  await writeFile(path.join(server.projectDir, 'unrelated.qmd'), '# Still unrelated\n');
  await page.waitForTimeout(2500);
  expect(await observedUpdates(page, 'unrelated.qmd')).toBe(unrelatedBefore);
});

test('single-file preview recovers when a missing include is created later', async ({ page }) => {
  await server.stop();
  server = await startPreviewServer({
    targetFile: 'index.qmd',
    fixtureFiles: [
      { path: 'index.qmd', content: '# Recover missing include\n\n{{< include later.qmd >}}\n' },
    ],
  });
  await page.goto(observedUrl(server.url));
  await page.waitForFunction(
    () => ((window as unknown as { __renderTicks?: number }).__renderTicks ?? 0) > 0,
    null,
    { timeout: 30_000 },
  );
  await writeFile(path.join(server.projectDir, 'later.qmd'), 'DYNAMIC-MISSING-RECOVERED\n');
  await waitForInnerText(page, 'DYNAMIC-MISSING-RECOVERED');
});

// Index retirement must invalidate even if the deleted file never emits a
// content event from the boot-time watcher.
test('single-file preview invalidates a deleted include and recovers on re-creation', async ({ page }) => {
  await server.stop();
  server = await startPreviewServer({
    targetFile: 'index.qmd',
    fixtureFiles: [
      { path: 'index.qmd', content: '# Deletion test\n\n{{< include part.qmd >}}\n' },
      { path: 'part.qmd', content: 'INCLUDED-BEFORE-DELETION\n' },
    ],
  });
  await page.goto(observedUrl(server.url));
  await waitForInnerText(page, 'INCLUDED-BEFORE-DELETION');
  const before = await ticks(page);
  await unlink(path.join(server.projectDir, 'part.qmd'));
  await page.waitForFunction(
    (oldTicks) => ((window as unknown as { __renderTicks?: number }).__renderTicks ?? 0) > oldTicks,
    before,
    { timeout: 30_000 },
  );
  // A render attempt alone is not proof of correct invalidation: it must
  // actually report the broken include before we repair the source.
  await expect(page.locator('.preview-error-overlay--error')).toBeVisible();
  await page.locator('.preview-error-expand-btn').click();
  await expect(page.locator('.preview-error-message')).toContainText(/part\.qmd|include/i);
  await writeFile(path.join(server.projectDir, 'part.qmd'), 'INCLUDED-AFTER-RECREATION\n');
  await waitForInnerText(page, 'INCLUDED-AFTER-RECREATION');
  await expect(page.locator('.preview-error-overlay--error')).toHaveCount(0);
});

test('single-file preview retires an include removed from the document', async ({ page }) => {
  await server.stop();
  server = await startPreviewServer({
    targetFile: 'index.qmd',
    fixtureFiles: [
      { path: 'index.qmd', content: '# Retirement test\n\n{{< include old.qmd >}}\n' },
      { path: 'old.qmd', content: 'OLD-DEPENDENCY-VISIBLE\n' },
    ],
  });
  await page.goto(observedUrl(server.url));
  await waitForInnerText(page, 'OLD-DEPENDENCY-VISIBLE');
  await writeFile(path.join(server.projectDir, 'index.qmd'), '# Retirement test\n\nNo more include\n');
  await waitForInnerText(page, 'No more include');
  await page.waitForTimeout(3000); // At least one refresh interval before asserting retirement.
  const oldCount = await observedUpdates(page, 'old.qmd');
  await writeFile(path.join(server.projectDir, 'old.qmd'), 'OLD-DEPENDENCY-CHANGED\n');
  await page.waitForTimeout(3000);
  expect(await observedUpdates(page, 'old.qmd')).toBe(oldCount);
});
