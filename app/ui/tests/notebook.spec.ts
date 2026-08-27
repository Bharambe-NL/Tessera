/**
 * The Notebook. Doc 16 sections 2.1 and 3.4.
 *
 * The view exists for one promise: an answer that found nothing in your own
 * notes says so, visibly. So what these drive is the grounded case, the
 * ungrounded case, and the two verbs that take a turn somewhere else.
 */

import { expect, test } from '@playwright/test';

import { freshCore, useCore, vaultCore } from './shell.js';

/**
 * Open the Notebook on a core with a vault, or on one without.
 *
 * The two states worth driving need different vaults, not different questions:
 * a lexical index over one page answers most things somewhat, so the honest way
 * to reach the ungrounded state is a profile whose vault is empty, which is
 * also the state most people's first question meets.
 */
async function openNotebook(
  page: import('@playwright/test').Page,
  vault: 'with' | 'without',
): Promise<void> {
  if (vault === 'with') await vaultCore(page);
  else await freshCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await page.locator('#rail [data-view="notebook"]').click();
  await expect(page.locator('#page-title')).toHaveText('Notebook');
}

test('an empty notebook says what it is for', async ({ page }) => {
  await openNotebook(page, 'without');
  await expect(page.locator('#page-body .page-empty')).toContainText('your own notes');
  await expect(page.locator('[data-notebook-act="new"]')).toBeVisible();
});

test('a question the vault answers is marked as coming from the notes', async ({ page }) => {
  // The dev server's fixture writes one page about world models, so this is a
  // question the vault can answer and the corpus is not asked.
  await openNotebook(page, 'with');
  await page.locator('[data-notebook-act="new"]').click();
  await expect(page.locator('#notebook-question')).toBeVisible({ timeout: 30_000 });

  await page.locator('#notebook-question').fill('what are world models?');
  await page.locator('#notebook-ask button[type="submit"]').click();

  const turn = page.locator('.turn').first();
  await expect(turn.locator('.asked')).toContainText('world models', { timeout: 60_000 });
  await expect(turn.locator('.chip.grounded, .chip.partly')).toBeVisible({ timeout: 60_000 });
  // Doc 16 section 3.4: the answer names what it read.
  await expect(turn.locator('.notebook-sources li')).not.toHaveCount(0);
});

test('a question the vault cannot answer says so and offers the way out', async ({ page }) => {
  // Doc 16 section 2.1's whole point, on the profile most people start from:
  // an empty vault. The way out is doc 05 section 8.1's web retriever, live
  // since 13e, and it says what it needs when the profile has named no source
  // rather than failing silently.
  await openNotebook(page, 'without');
  await page.locator('[data-notebook-act="new"]').click();
  await expect(page.locator('#notebook-question')).toBeVisible({ timeout: 30_000 });

  await page.locator('#notebook-question').fill('what did I write about world models?');
  await page.locator('#notebook-ask button[type="submit"]').click();

  const turn = page.locator('.turn').first();
  await expect(turn.locator('.chip.ungrounded')).toBeVisible({ timeout: 60_000 });
  await expect(turn.locator('.page-note')).toContainText('nothing on this');
  const web = turn.locator('[data-notebook-act="search-web"]');
  await expect(web).toBeEnabled();

  // The dev profile has named no web source, so the one click way out says
  // where to set one up rather than reaching a socket nobody pointed it at.
  await web.click();
  await expect(page.locator('#toasts')).toContainText('Profile', { timeout: 30_000 });
});

test('a turn is kept as a page and opened on a board', async ({ page }) => {
  await openNotebook(page, 'with');
  await page.locator('[data-notebook-act="new"]').click();
  await page.locator('#notebook-question').fill('what are world models?');
  await page.locator('#notebook-ask button[type="submit"]').click();

  const turn = page.locator('.turn').first();
  await expect(turn.locator('.verbs')).toBeVisible({ timeout: 60_000 });

  await turn.locator('[data-notebook-act="save"]').click();
  await expect(turn.locator('.chip.page')).toBeVisible({ timeout: 60_000 });

  // And the same question grows into a board, which is where follow-ups live.
  await turn.locator('[data-notebook-act="open-board"]').click();
  await expect(page.locator('#page')).toBeHidden({ timeout: 60_000 });
  await expect(page.locator('#cards .card .answer')).toBeVisible({ timeout: 60_000 });
});
