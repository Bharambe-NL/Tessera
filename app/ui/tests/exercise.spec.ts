/**
 * The exercise, driven end to end. Doc 08 and doc 11 section 6.
 *
 * The two deterministic checks are asserted in Rust and re-checked in Python by
 * the scorer, so what this covers is the part neither can: that a reader can
 * open one, answer it, be graded, and reach the card an item came from.
 */

import { expect, test } from '@playwright/test';

import { freshCore, useCore } from './shell.js';

test.beforeEach(async ({ page }) => {
  await freshCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
});

async function askOne(page: import('@playwright/test').Page, q: string): Promise<void> {
  await page.locator('#ask').fill(q);
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer').last()).toBeVisible({ timeout: 30_000 });
}

test('an exercise asks, grades, and points back at the card', async ({ page }) => {
  await askOne(page, 'what are world models?');

  await page.locator('#check').click();
  const sheet = page.locator('#exercise');
  await expect(sheet).toBeVisible();

  const items = page.locator('.ex-item');
  await expect(items).toHaveCount(1, { timeout: 30_000 });

  // Doc 08 section 9 admits every item, so nothing is marked before grading:
  // a mark shown earlier would answer the question.
  await expect(page.locator('.opt.right')).toHaveCount(0);
  await expect(page.locator('#ex-submit')).toBeDisabled();

  // Answer it wrong on purpose, so the grade is a fact rather than a default.
  await items.first().locator('.opt').nth(1).click();
  await expect(page.locator('#ex-submit')).toBeEnabled();
  await page.locator('#ex-submit').click();

  await expect(page.locator('.ex-foot .score')).toContainText('0');
  await expect(page.locator('.opt.right')).toHaveCount(1);
  await expect(page.locator('.opt.wrong')).toHaveCount(1);
  // The correct option is the one lifted from the card, which is the whole of
  // doc 08 section 5's traceability rule seen from the reader's side.
  const correct = await page.locator('.opt.right').innerText();
  const answer = await page.locator('#cards .card .answer').first().innerText();
  expect(answer).toContain(correct.trim());

  // Doc 08 section 11: the item links to its source card.
  await page.locator('[data-item-act="open"]').click();
  await expect(sheet).toBeHidden();
  await expect(page.locator('#cards .card')).toHaveCount(1);
});

test('answering right scores right', async ({ page }) => {
  await askOne(page, 'what are world models?');
  await page.locator('#check').click();
  await expect(page.locator('.ex-item')).toHaveCount(1, { timeout: 30_000 });

  await page.locator('.ex-item').first().locator('.opt').first().click();
  await page.locator('#ex-submit').click();
  await expect(page.locator('.ex-foot .score')).toContainText('1');
  await expect(page.locator('.opt.wrong')).toHaveCount(0);
});

test('a board with nothing checked says so', async ({ page }) => {
  // Doc 08 section 10's `no_eligible_cards`, in the reader's words rather than
  // as an empty list they have to interpret.
  await page.locator('#check').click();
  await expect(page.locator('#exercise .page-empty')).toContainText('nothing to test', {
    timeout: 30_000,
  });
  await expect(page.locator('.ex-item')).toHaveCount(0);
});

test('reporting an item records it and leaves the exercise alone', async ({ page }) => {
  await askOne(page, 'what are world models?');
  await page.locator('#check').click();
  await expect(page.locator('.ex-item')).toHaveCount(1, { timeout: 30_000 });
  await page.locator('.ex-item').first().locator('.opt').first().click();
  await page.locator('#ex-submit').click();

  await page.locator('[data-item-act="report"]').click();
  await expect(page.locator('#toasts')).toContainText('Reported');
  // Doc 08 section 11: a report is a record for pack maintenance, not an edit.
  await expect(page.locator('.ex-item')).toHaveCount(1);
});

test('escape closes the exercise', async ({ page }) => {
  await askOne(page, 'what are world models?');
  await page.locator('#check').click();
  await expect(page.locator('.ex-item')).toHaveCount(1, { timeout: 30_000 });
  await page.locator('#exercise').press('Escape');
  await expect(page.locator('#exercise')).toBeHidden();
});
