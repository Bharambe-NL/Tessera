/**
 * A card that builds on a card from another board. Doc 12's walkthrough line 15.
 *
 * "A card on a second board builds on a verified card from the first, citing
 * the original source."
 *
 * The core has done this since M6 and a Rust test has proved it since then.
 * What was missing until M13 was any screen that said so: `builds_on` was
 * recorded, carried over the RPC, and rendered nowhere. This drives the whole
 * row from the outside, which is the only way to tell those two apart.
 */

import { expect, test, type Page } from '@playwright/test';

import { memoryCore, useCore } from './shell.js';

async function askDeep(page: Page, question: string): Promise<void> {
  // Doc 15 section 3: only a deep or research card is eligible to be
  // remembered, so a fast one would leave nothing for the second board to find.
  await page.locator('#modes button', { hasText: 'deep' }).click();
  await page.locator('#ask').fill(question);
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer').last()).toBeVisible({ timeout: 60_000 });
}

test('a card on a second board says which prior card it was built on', async ({ page }) => {
  await memoryCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');

  await askDeep(page, 'what are world models?');

  // A second board, made the way a person makes one.
  await page.locator('.rail-item[data-view="home"]').click();
  await page.locator('#home-create').click();
  await expect(page.locator('#cards .card')).toHaveCount(0);

  await askDeep(page, 'how does a world model predict?');

  // Doc 09 section 4's disclosure is where the audit trail lives, so that is
  // where the prior card has to be named.
  await page.locator('.card details.built summary').last().click();
  const trail = page.locator('.built-body').last();
  await expect(trail.locator('.built-row')).not.toHaveCount(0, { timeout: 30_000 });
  await expect(trail).toContainText('Builds on', { timeout: 30_000 });
});
