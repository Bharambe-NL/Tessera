/**
 * The rail and the four pages. Doc 11 sections 5 and 6.
 *
 * These drive the real product against a real core, so a page that renders from
 * a read the core does not serve fails here rather than at first use. The
 * lesson from step 1 holds: counting rows is not enough, so where a verb writes
 * something these assert what the write did.
 */

import { expect, test } from '@playwright/test';

import { freshCore, useCore } from './shell.js';

const QUESTION = 'what are world models?';

test.beforeEach(async ({ page }) => {
  await freshCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
});

async function askOne(page: import('@playwright/test').Page, question = QUESTION): Promise<void> {
  await page.locator('#ask').fill(question);
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer')).toBeVisible({ timeout: 30_000 });
}

test('the rail opens each page and the board keeps its state underneath', async ({ page }) => {
  await askOne(page);

  for (const [view, title] of [
    ['home', 'Home'],
    ['flags', 'Flags'],
    ['library', 'Library'],
    ['profile', 'Profile'],
  ] as const) {
    await page.locator(`#rail [data-view="${view}"]`).click();
    await expect(page.locator('#page')).toBeVisible();
    await expect(page.locator('#page-title')).toHaveText(title);
    await expect(page.locator(`#rail [data-view="${view}"]`)).toHaveAttribute('aria-current', 'page');
  }

  // A page covers the canvas rather than replacing it, so the card is still
  // there and the board did not reload.
  await page.locator('#rail [data-view="board"]').click();
  await expect(page.locator('#page')).toBeHidden();
  await expect(page.locator('#cards .card')).toHaveCount(1);
});

test('opening the rail moves the page rather than covering it', async ({ page }) => {
  await askOne(page);
  await page.locator('#rail [data-view="flags"]').click();
  await expect(page.locator('.flag-row')).toHaveCount(1);

  const box = async (selector: string) => {
    const b = await page.locator(selector).boundingBox();
    if (!b) throw new Error(`${selector} has no box`);
    return b;
  };

  // Collapsed, the row starts clear of the rail.
  expect((await box('.flag-row')).x).toBeGreaterThanOrEqual((await box('#rail')).width);

  await page.locator('#rail-toggle').click();
  await page.waitForTimeout(500);

  // Open, it still does. Visibility is not the test: a covered element is
  // visible and clickable to a driver that scrolls it into view, so the first
  // version of this suite passed while the open rail sat on top of every row's
  // severity chip, checkbox and rule name.
  const rail = await box('#rail');
  expect(rail.width).toBeGreaterThan(200);
  const row = await box('.flag-row');
  expect(row.x).toBeGreaterThanOrEqual(rail.x + rail.width);
  expect((await box('.flag-row .chip.sev')).x).toBeGreaterThanOrEqual(rail.x + rail.width);
  expect((await box('.flag-group h2')).x).toBeGreaterThanOrEqual(rail.x + rail.width);
});

test('home lists the board, and trash is a filter rather than a page', async ({ page }) => {
  await askOne(page);
  await page.locator('#rail [data-view="home"]').click();

  const card = page.locator('.board-card');
  await expect(card).toHaveCount(1);
  await expect(card.locator('h2')).toHaveText(QUESTION);
  // `board.list` already returned the open flag count, so the grid needed no
  // new read. A fast card carries `fast_mode_notice`, so the chip is there.
  await expect(card.locator('.chip.flag')).toHaveText('1');

  await card.locator('[data-board-act="trash"]').click();
  await expect(page.locator('.board-card')).toHaveCount(0);
  await expect(page.locator('.page-empty')).toBeVisible();

  // Doc 09 open question 1, adopted by doc 11: the same grid, one word apart.
  await page.locator('[data-home-filter="trashed"]').click();
  await expect(page.locator('.board-card')).toHaveCount(1);

  await page.locator('[data-board-act="restore"]').click();
  await expect(page.locator('.board-card')).toHaveCount(0);
  await page.locator('[data-home-filter="active"]').click();
  await expect(page.locator('.board-card')).toHaveCount(1);
});

test('the flags queue spans boards and a decision takes the row away', async ({ page }) => {
  await askOne(page);
  // A second board, so the queue has something to group.
  await page.locator('#rail [data-view="home"]').click();
  await page.locator('#home-create').click();
  await expect(page.locator('#page')).toBeHidden();
  await askOne(page, 'and what about the buffer?');

  await page.locator('#rail [data-view="flags"]').click();
  await expect(page.locator('.flag-group')).toHaveCount(2, { timeout: 30_000 });

  const rows = page.locator('.flag-row');
  await expect(rows).toHaveCount(2);
  // Doc 09 section 6: every row carries a severity chip, the rule, the card it
  // is on and its age.
  const first = rows.first();
  await expect(first.locator('.chip.sev')).toBeVisible();
  await expect(first.locator('.rule')).not.toBeEmpty();
  await expect(first.locator('.card-title')).not.toBeEmpty();
  await expect(first.locator('.age')).not.toBeEmpty();
  await expect(first.locator('.reason')).not.toBeEmpty();

  await first.locator('[data-flag-act="dismiss"]').click();
  await expect(page.locator('.flag-row')).toHaveCount(1);

  // The rail badge follows, because it is the only part of the queue visible
  // from the board.
  await expect(page.locator('#rail-flags')).toHaveText('1');
});

test('bulk dismiss asks twice and bulk accept does not', async ({ page }) => {
  await askOne(page);
  await page.locator('#rail [data-view="flags"]').click();
  await expect(page.locator('.flag-row')).toHaveCount(1);

  await page.locator('.flag-row .pick').check();
  await expect(page.locator('.bulk .count')).toHaveText('1 selected');

  // Doc 09 section 6: bulk Dismiss requires a second click with the count shown.
  await page.locator('[data-bulk="dismiss"]').click();
  await expect(page.locator('[data-bulk="dismiss-confirm"]')).toContainText('1');
  await expect(page.locator('.flag-row')).toHaveCount(1, { timeout: 2000 });

  await page.locator('[data-bulk="dismiss-confirm"]').click();
  await expect(page.locator('.flag-row')).toHaveCount(0);
  await expect(page.locator('#rail-flags')).toBeHidden();
});

test('library shows both tabs and says what is missing', async ({ page }) => {
  await askOne(page);
  await page.locator('#rail [data-view="library"]').click();

  // A fast card retrieves nothing, so both tabs are honestly empty and say what
  // would put something in them.
  await expect(page.locator('.page-empty')).toContainText('No sources yet');
  await page.locator('[data-library-tab="concepts"]').click();
  await expect(page.locator('.page-empty')).toContainText('No concepts yet');
  await page.locator('[data-library-tab="sources"]').click();
  await expect(page.locator('[data-library-tab="sources"]')).toHaveClass(/on/);
});

test('the profile models page reports key presence and never a key', async ({ page }) => {
  await page.locator('#rail [data-view="profile"]').click();
  await expect(page.locator('.facts')).toBeVisible();

  await page.locator('[data-profile-tab="models"]').click();
  const rows = page.locator('.lib-row[data-key-ref]');
  await expect(rows.first()).toBeVisible();

  // Doc 10 section 8 and the standing rule. The dev server holds a fake key
  // whose value is known, so this is a real check that it never crossed.
  const html = await page.locator('#page-body').innerHTML();
  expect(html).not.toContain('sk-');
  await expect(page.locator('.page-note')).toContainText('keychain');

  await page.locator('[data-profile-tab="diagnostics"]').click();
  await expect(page.locator('.facts')).toContainText('Events');

  await page.locator('[data-profile-tab="doctrine"]').click();
  await expect(page.locator('.facts')).toContainText('general');
});
