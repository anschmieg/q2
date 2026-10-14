/**
 * Website search end to end (bd-eip0naf4).
 *
 * Renders a small website with `q2 preview --static` (the `q2 render`
 * path, so the Pagefind bundle, the search scripts, and the navbar
 * box are all real) and drives the search UI in the browser:
 *
 *   1. The overlay opens from the navbar button, a query returns the
 *      page grouped with its matching section and a highlighted
 *      excerpt, and following the result lands on the page with the
 *      `?q=` term marked (and the query stripped from the URL).
 *   2. The keyboard shortcut reopens the overlay.
 *   3. A site without a navbar renders the textbox in the sidebar and
 *      its results panel in `#quarto-search-results`.
 *
 * Deliberately not pinned: Pagefind's ranking and excerpt wording.
 */

import { test, expect } from '@playwright/test';
import { startPreviewServer, type FixtureFile, type PreviewServerHandle } from './helpers/previewServer';

const PAGES: FixtureFile[] = [
  {
    path: 'index.qmd',
    content: `---
title: Home
---

Intro paragraph before any section.

## First section

Text in first section with \`code\`.

### Nested heading

Nested text about marmots.

## Second section

More text.
`,
  },
  {
    path: 'docs/guide.qmd',
    content: `---
title: Guide page
---

Guide page body about rendering.
`,
  },
];

const NAVBAR_SITE: FixtureFile[] = [
  {
    path: '_quarto.yml',
    content: `project:
  type: website
  output-dir: _site
website:
  title: Search E2E
  navbar:
    left:
      - index.qmd
      - docs/guide.qmd
`,
  },
  ...PAGES,
];

const SIDEBAR_SITE: FixtureFile[] = [
  {
    path: '_quarto.yml',
    content: `project:
  type: website
  output-dir: _site
website:
  title: Search E2E
  sidebar:
    contents:
      - index.qmd
      - docs/guide.qmd
`,
  },
  ...PAGES,
];

test.describe('website search', () => {
  let server: PreviewServerHandle | undefined;

  test.afterEach(async () => {
    await server?.stop();
    server = undefined;
  });

  test('navbar overlay: query, grouped results, ?q= highlight, shortcut', async ({ page }) => {
    server = await startPreviewServer({ fixtureFiles: NAVBAR_SITE, staticMode: true });
    // `--no-watch` serves no reload events; Firefox logs the SSE
    // client's failed connection as a console error (Chromium stays
    // silent). That is the preview's business, not search's.
    const isReloadNoise = (text: string) => text.includes('__q2-preview/events');
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    page.on('console', (m) => {
      if (m.type() === 'error' && !isReloadNoise(m.text())) errors.push(m.text());
    });

    await page.goto(server.url + 'index.html');
    const box = page.locator('#quarto-search');
    await expect(box).toHaveClass(/type-overlay/);
    await box.locator('.aa-DetachedSearchButton').click();
    const input = page.locator('.aa-DetachedFormContainer .aa-Input');
    await input.fill('marmots');

    const docs = page.locator('.search-result-doc');
    await expect(docs).toHaveCount(1);
    await expect(page.locator('.search-result-header')).toHaveText(/1 matching documents/);
    await expect(page.locator('.search-result-title').first()).toHaveText('Home');
    await expect(page.locator('.search-result-section')).toContainText(['Nested heading']);
    await expect(page.locator('.search-result-text mark.search-match').first()).toBeVisible();

    const href = await page.locator('.search-result-link').first().getAttribute('href');
    expect(href).toContain('?q=marmots');
    await page.locator('.search-result-link').first().click();
    await page.waitForLoadState('domcontentloaded');
    // The query is stripped from the URL at module load; the match is marked.
    expect(page.url()).not.toContain('?q=');
    await expect(page.locator('main mark').first()).toHaveText(/marmots/i);

    await page.keyboard.press('f');
    await expect(page.locator('.aa-DetachedFormContainer .aa-Input')).toBeVisible();

    expect(errors).toEqual([]);
  });

  test('sidebar-only site: textbox with the panel in #quarto-search-results', async ({ page }) => {
    server = await startPreviewServer({ fixtureFiles: SIDEBAR_SITE, staticMode: true });
    await page.goto(server.url + 'index.html');
    const box = page.locator('#quarto-sidebar .sidebar-search #quarto-search');
    await expect(box).toHaveClass(/type-textbox/);
    const input = box.locator('.aa-Input');
    await input.click();
    await input.fill('rendering');
    const result = page.locator('#quarto-search-results .search-result-doc');
    await expect(result).toHaveCount(1);
    await expect(result.locator('.search-result-title')).toHaveText('Guide page');
  });
});
