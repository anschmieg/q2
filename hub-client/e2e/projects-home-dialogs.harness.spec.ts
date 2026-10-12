/**
 * Interaction spec for the ProjectsHome dialogs after their conversion to
 * the shared ModalDialog (Phase 1): opened through the real UI (New menu,
 * Connect/Import button), each must present the WCAG dialog contract —
 * role="dialog", aria-modal, labelled by its title, Escape closes, focus
 * returns to the trigger, and the header close button works.
 *
 * Runs in the no-server visual config (DevHarness, not screenshots).
 */

import { test, expect } from '@playwright/test';
import { bootHarness, openNewProjectMenu } from './helpers/harness';

test.beforeEach(async ({ page }) => {
  await bootHarness(page, 'projects-home', '.projects-home', 'light');
});

test('New project dialog opens via the New menu and closes with focus return', async ({
  page,
}) => {
  const trigger = page.getByRole('button', { name: '＋ New ▾' });
  // openNewProjectMenu waits for the registry's groups to replace the placeholder item
  // (see its docstring). The Menu keeps keyboard focus on its first item through that swap,
  // so the Templates group is focused once it exists. (Pressing Enter on whatever was focused
  // first is what flaked: the placeholder could vanish between the focus check and the key,
  // taking focus with it; bd-c72wsugj.)
  const menu = await openNewProjectMenu(page);
  const templates = menu.getByRole('menuitem', { name: /^Templates/ });
  await expect(templates).toBeFocused();

  // Keyboard: ArrowRight opens the group and focuses its first choice; Enter opens the dialog.
  await page.keyboard.press('ArrowRight');
  await expect(menu.getByRole('menuitem', { name: /^Default/ })).toBeFocused();
  await page.keyboard.press('Enter');

  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute('aria-modal', 'true');
  await expect(dialog.locator('h2')).toHaveText('New default');
  // Focus is inside the dialog (the name input autofocuses).
  await expect(dialog.locator('#qh-new-name')).toBeFocused();

  await page.keyboard.press('Escape');
  await expect(page.locator('[role="dialog"]')).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

test('Connect/Import dialog: header close button closes and returns focus', async ({ page }) => {
  const trigger = page.locator('button:has-text("Connect / Import")');
  await trigger.click();

  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible();
  await expect(dialog.locator('h2')).toHaveText('Add an existing project');

  // Tab cycles within the dialog (focus trap): Shift+Tab from the first
  // focusable wraps to the last.
  await dialog.locator('.close-btn').focus();
  await page.keyboard.press('Shift+Tab');
  const focused = await page.evaluate(() => document.activeElement?.textContent);
  // The Import submit is disabled until a file is chosen, so the last
  // enabled focusable is Cancel.
  expect(focused).toBe('Cancel');

  await dialog.locator('.close-btn').click();
  await expect(page.locator('[role="dialog"]')).toHaveCount(0);
  await expect(trigger).toBeFocused();
});
