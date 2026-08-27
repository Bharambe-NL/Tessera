/**
 * The Pages view. Doc 16 section 3.7 and phase 12c.
 *
 * The vault is the half of the product a person writes rather than asks, so
 * these drive it the way somebody would: keep a card, write a page, link one to
 * another, follow the link, and write the page a link named before it existed.
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

async function openPages(page: import('@playwright/test').Page): Promise<void> {
  await page.locator('#rail [data-view="pages"]').click();
  await expect(page.locator('#page-title')).toHaveText('Pages');
}

test('the vault starts empty and says what to do about it', async ({ page }) => {
  await openPages(page);
  await expect(page.locator('#page-body .page-empty')).toContainText('Keep a card as a page');
});

test('a page is written, read back, and edited under a new title', async ({ page }) => {
  await openPages(page);
  await page.locator('[data-page-act="new"]').click();

  await page.locator('#page-name').fill('Liquidity risk');
  await page.locator('#page-text').fill('# Liquidity risk\n\nThe rule is in article 12.');
  await page.locator('#page-edit button[type="submit"]').click();

  // Written, and shown as the page rather than as the markdown.
  await expect(page.locator('.page-read h2')).toHaveText('Liquidity risk', { timeout: 30_000 });
  await expect(page.locator('.page-read p')).toContainText('article 12');
  await expect(page.locator('.page-file')).toContainText('vault/liquidity-risk.md');

  // A rename keeps the page and moves the file.
  await page.locator('[data-page-act="edit"]').click();
  await page.locator('#page-name').fill('Liquidity coverage');
  await page.locator('#page-edit button[type="submit"]').click();
  await expect(page.locator('.page-file')).toContainText('vault/liquidity-coverage.md', {
    timeout: 30_000,
  });

  await page.locator('[data-page-act="close"]').click();
  await expect(page.locator('.lib-row .title')).toHaveText('Liquidity coverage');
});

test('a link to a page that does not exist writes it, and the backlink appears', async ({
  page,
}) => {
  // Doc 16 section 3.1: an unresolved link is kept and creates the page on
  // click. Doc 16 section 2.1: the backlink is a query over the links.
  await openPages(page);
  await page.locator('[data-page-act="new"]').click();
  await page.locator('#page-name').fill('Reading notes');
  await page.locator('#page-text').fill('I should read [[Basel III|the accord]].');
  await page.locator('#page-edit button[type="submit"]').click();

  const link = page.locator('.wikilink.unresolved');
  await expect(link).toHaveText('the accord', { timeout: 30_000 });

  await link.click();
  // It lands in the new page, ready to write.
  await expect(page.locator('#page-name')).toHaveValue('Basel III', { timeout: 30_000 });
  await page.locator('#page-text').fill('# Basel III\n\nThe accord.');
  await page.locator('#page-edit button[type="submit"]').click();

  // And the page that named it now points at it.
  await expect(page.locator('.page-read')).toContainText('The accord.', { timeout: 30_000 });
  await expect(page.locator('.lib-list .title')).toHaveText('Reading notes');
});

test('a kept card appears in the pages with its citations counted', async ({ page }) => {
  // Doc 16 section 3.2: the page carries the card's citations, and the explorer
  // says so without opening it, because that is the difference between a page
  // that can support a claim and one that is only context.
  await page.locator('#ask').fill(QUESTION);
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer')).toBeVisible({ timeout: 30_000 });
  await page.locator('#cards .card .save').first().click();
  await expect(page.locator('#cards .card .chip.page')).toBeVisible({ timeout: 30_000 });

  await openPages(page);
  const row = page.locator('.lib-row').first();
  await expect(row.locator('.title')).toContainText('world models');
  // The dev server's first card is answered from model knowledge and cites
  // nothing, so the chip says where the page came from rather than counting a
  // zero. The citations that a card with sources carries are asserted where
  // there are sources to carry, in `end_to_end.rs`.
  await expect(row.locator('.chip')).toContainText('from a card');

  await row.locator('[data-page-act="open"]').click();
  await expect(page.locator('.page-read')).toContainText('world model', { timeout: 30_000 });
  await expect(page.locator('.page-read h3').first()).toHaveText('What it said');
});

test('a page is removed and the vault forgets it', async ({ page }) => {
  await openPages(page);
  await page.locator('[data-page-act="new"]').click();
  await page.locator('#page-name').fill('A passing thought');
  await page.locator('#page-text').fill('Nothing to keep.');
  await page.locator('#page-edit button[type="submit"]').click();
  await expect(page.locator('.page-read')).toBeVisible({ timeout: 30_000 });

  await page.locator('[data-page-act="close"]').click();
  await page.locator('.lib-row [data-page-act="remove"]').click();
  await expect(page.locator('#page-body .page-empty')).toBeVisible({ timeout: 30_000 });
});
