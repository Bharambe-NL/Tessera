/**
 * Doc 12 phase 8: "every verb in 09 section 5 works and emits its event".
 *
 * These are the verbs M9 step 1 wires. A rendered screen and a working one are
 * different claims, and the gap between them is exactly what this file exists to
 * close: `render.ts` emitted `data-act="flags"`, `data-act="remove"` and
 * `data-act="follow"` for four milestones with no listener anywhere, so the
 * markup looked finished and nothing on the card responded.
 */

import { expect, test } from '@playwright/test';

import { cardCount, freshCore, useCore } from './shell.js';

const QUESTION = 'what are world models?';

test.beforeEach(async ({ page }) => {
  await freshCore(page);
  await useCore(page);
});

/** Ask from the composer and wait for the card the core wrote. */
async function askFirst(page: import('@playwright/test').Page): Promise<void> {
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');

  await page.locator('#ask').fill(QUESTION);
  await page.locator('#ask').press('Enter');

  await expect(page.locator('#cards .card')).toHaveCount(1, { timeout: 30_000 });
  await expect(page.locator('#cards .card .answer')).toBeVisible();
}

test('a question becomes a card the reader can see', async ({ page }) => {
  await askFirst(page);

  const card = page.locator('#cards .card').first();
  await expect(card.locator('.msg')).toHaveText(QUESTION);
  await expect(card.locator('.answer')).toContainText('world model');
  // The board takes its name from the first question. Doc 01 section 4.1.
  await expect(page.locator('#title')).toHaveValue(QUESTION);
});

test('the follow-up box on a card asks a follow-up', async ({ page }) => {
  await askFirst(page);
  const parent = page.locator('#cards .card').first();
  const parentId = await parent.getAttribute('data-card-id');

  await parent.locator('.followup').fill('which article says so?');
  await parent.locator('.followup').press('Enter');

  await expect(page.locator('#cards .card')).toHaveCount(2, { timeout: 30_000 });

  // The follow-up card answered. Counting cards is not enough: a card that
  // renders and fails is still a card, and the first version of this test
  // passed against a follow-up carrying "This card did not finish."
  const child = page.locator('#cards .card').nth(1);
  await expect(child).not.toHaveAttribute('data-status', 'failed');
  await expect(child.locator('.answer')).toContainText('world model');
  await expect(child.locator('.failed')).toHaveCount(0);

  // It names its parent rather than landing on the board as another root. This
  // is the assertion the RPC could not satisfy before M9: `card.ask` passed
  // `parent_card_id: None`.
  const cards = page.locator('#cards .card');
  const titles = await cards.locator('.head .title').allTextContents();
  expect(titles).toContain('Follow-up');

  const ids = await cards.evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.cardId));
  expect(ids.filter((id) => id === parentId)).toHaveLength(1);

  // An edge is drawn from parent to child, which is what makes it a board
  // rather than a list.
  const followEdge = await page.locator('#edges .edge.follow').getAttribute('d');
  expect(followEdge?.length ?? 0).toBeGreaterThan(0);
});

test('the flag chip opens the reason and closes it again', async ({ page }) => {
  await askFirst(page);
  const card = page.locator('#cards .card').first();

  // A fast card carries `fast_mode_notice`, so the chip is always there on this
  // board. Doc 07 section B8.1.
  const chip = card.locator('.chip.flag');
  await expect(chip).toBeVisible();
  await expect(chip).toHaveAttribute('aria-expanded', 'false');
  await expect(card.locator('.flag-list')).toHaveCount(0);

  await chip.click();
  await expect(card.locator('.flag-list li')).not.toHaveCount(0);
  await expect(card.locator('.flag-list .rule').first()).not.toBeEmpty();
  await expect(card.locator('.flag-list .reason').first()).not.toBeEmpty();

  await card.locator('.chip.flag').click();
  await expect(card.locator('.flag-list')).toHaveCount(0);
});

test('rerun checks the card again without writing a second one', async ({ page }) => {
  await askFirst(page);
  const before = await cardCount(page);
  const card = page.locator('#cards .card').first();
  const id = await card.getAttribute('data-card-id');

  await card.locator('[data-act="rerun"]').click();
  await expect(page.locator('#mode-label')).toHaveText('Live', { timeout: 30_000 });

  // Doc 09 section 5's Rerun on a card retrieves nothing and rewrites nothing.
  expect(await cardCount(page)).toBe(before);
  await expect(page.locator('#cards .card').first()).toHaveAttribute('data-card-id', id ?? '');
});

test('renaming the board stops the next question renaming it', async ({ page }) => {
  await askFirst(page);

  await page.locator('#title').fill('Capital rules');
  await page.locator('#title').press('Enter');

  await page.locator('#ask').fill('and what about the buffer?');
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card')).toHaveCount(2, { timeout: 30_000 });
  await expect(page.locator('#cards .card').nth(1)).not.toHaveAttribute('data-status', 'failed');

  // Doc 01 section 4.1: the inference stops once a person has typed a title.
  await expect(page.locator('#title')).toHaveValue('Capital rules');
});

test('no card on the board reports a failure', async ({ page }) => {
  // The guard the other tests needed and did not have. A card that renders is
  // not a card that answered, and every assertion above counts cards or reads
  // one field; this one reads every card's terminal state and every error the
  // shell surfaced, so a fixture that runs dry fails here first.
  await askFirst(page);
  const parent = page.locator('#cards .card').first();

  for (const question of ['which article says so?', 'and the buffer?']) {
    await parent.locator('.followup').fill(question);
    await parent.locator('.followup').press('Enter');
    await expect(page.locator('#mode-label')).toHaveText('Live', { timeout: 30_000 });
  }

  expect(await cardCount(page)).toBe(3);
  await expect(page.locator('#cards .card[data-status="failed"]')).toHaveCount(0);
  await expect(page.locator('#cards .failed')).toHaveCount(0);
  await expect(page.locator('#toasts .toast.error')).toHaveCount(0);
});
