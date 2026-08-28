/**
 * Doc 01 section 4.2: a card's `position` is a layout slot plus the user's
 * offset, and `pinned` says the person chose it.
 *
 * The fields and the layout that honours them shipped at M0. Nothing ever wrote
 * them, because the canvas had one drag and it panned the whole world, so every
 * card moved together and a board could not be arranged. These drive the drag
 * through the pointer rather than through the store, because a position that
 * only a test can set is not a board anybody can tidy.
 */

import { expect, test, type Page } from '@playwright/test';

import { freshCore, useCore } from './shell.js';

test.beforeEach(async ({ page }) => {
  await freshCore(page);
  await useCore(page);
});

/** Ask one question and wait for the card the core wrote. */
async function askOne(page: Page, question: string): Promise<void> {
  await page.locator('#ask').fill(question);
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer').last()).toBeVisible({ timeout: 30_000 });
  await settled(page);
}

/**
 * Wait until every card is drawn where its own transform says it is.
 *
 * Doc 11 section 7's card rise runs 360ms, and every card rises from the same
 * origin because `--rise-x` and `--rise-y` are not set per card. For those
 * 360ms two cards 560px apart are painted on top of each other, so a press
 * aimed at one lands on whichever is above it, and the drag moves the wrong
 * card.
 *
 * Asking the animations when they will finish is a race this lost: called in
 * the gap between the element being appended and the animation being
 * registered, `getAnimations` returns nothing and the wait ends before the rise
 * begins. Comparing the painted transform against the written one has no such
 * gap, because it tests the thing that actually has to be true.
 */
async function settled(page: Page): Promise<void> {
  await expect(async () => {
    const drift = await page.evaluate(() =>
      [...document.querySelectorAll('#cards .card')].map((el) => {
        const e = el as HTMLElement;
        const written = new DOMMatrixReadOnly(e.style.transform);
        const painted = new DOMMatrixReadOnly(getComputedStyle(e).transform);
        return Math.abs(written.m41 - painted.m41) + Math.abs(written.m42 - painted.m42);
      }),
    );
    expect(Math.max(0, ...drift)).toBeLessThan(1);
  }).toPass({ timeout: 10_000 });
}

/**
 * Where the card layer put a card, read off the transform it wrote.
 *
 * The inline transform rather than the computed one. A new card carries the
 * `rise` animation, which interpolates the computed transform for 360ms, so a
 * card measured just after a reload reports a point on the way to its position
 * rather than its position. `renderCards` and the drag both write the inline
 * one, so it is the answer at every moment.
 */
async function transformOf(page: Page, index = 0): Promise<{ x: number; y: number }> {
  return await page.locator('#cards .card').nth(index).evaluate((el) => {
    const m = new DOMMatrixReadOnly((el as HTMLElement).style.transform);
    return { x: Math.round(m.m41), y: Math.round(m.m42) };
  });
}

/**
 * Drag a card by its head.
 *
 * The steps matter. One jump from press to release never crosses the 3px the
 * handler uses to tell a drag from a click on the head, and the card would not
 * move at all.
 */
async function dragCardBy(page: Page, index: number, dx: number, dy: number): Promise<void> {
  const head = page.locator('#cards .card').nth(index).locator('.head');
  const box = await head.boundingBox();
  if (!box) throw new Error('the card head has no box to grab');
  const fromX = box.x + box.width / 2;
  const fromY = box.y + box.height / 2;

  await page.mouse.move(fromX, fromY);
  await page.mouse.down();
  await page.mouse.move(fromX + dx / 2, fromY + dy / 2, { steps: 8 });
  await page.mouse.move(fromX + dx, fromY + dy, { steps: 8 });
  await page.mouse.up();
}

test('a card goes where it is dragged and stays there', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await askOne(page, 'what are world models?');

  const before = await transformOf(page);
  await dragCardBy(page, 0, 160, 120);

  const after = await transformOf(page);
  expect(after.x).toBeGreaterThan(before.x + 100);
  expect(after.y).toBeGreaterThan(before.y + 80);

  // The point of the whole change. A move the core never heard about is a move
  // that lasts until the next read, which is the bug this closes.
  await page.reload();
  await expect(page.locator('#cards .card')).toHaveCount(1, { timeout: 30_000 });
  await settled(page);
  const reloaded = await transformOf(page);
  expect(reloaded.x).toBe(after.x);
  expect(reloaded.y).toBe(after.y);
});

test('one card moves and the others hold still', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await askOne(page, 'what are world models?');
  await askOne(page, 'how do they handle uncertainty?');
  await expect(page.locator('#cards .card')).toHaveCount(2);

  const otherBefore = await transformOf(page, 1);
  await dragCardBy(page, 0, 60, 200);

  // The complaint that started this: everything moved together, because the
  // only drag on the canvas panned the world.
  const otherAfter = await transformOf(page, 1);
  expect(otherAfter).toEqual(otherBefore);
});

test('tidy puts a moved card back, and that lasts too', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await askOne(page, 'what are world models?');

  const home = await transformOf(page);
  await dragCardBy(page, 0, 200, 150);
  expect(await transformOf(page)).not.toEqual(home);

  await page.locator('#tidy').click();
  await expect(async () => {
    expect(await transformOf(page)).toEqual(home);
  }).toPass({ timeout: 10_000 });

  // Tidy is the undo a drag has, so its release outlives the window the same
  // way the drag does.
  await page.reload();
  await expect(page.locator('#cards .card')).toHaveCount(1, { timeout: 30_000 });
  await settled(page);
  expect(await transformOf(page)).toEqual(home);
});

test('a press on the head that never travels is not a move', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await askOne(page, 'what are world models?');

  const before = await transformOf(page);
  await dragCardBy(page, 0, 1, 1);
  expect(await transformOf(page)).toEqual(before);
});
