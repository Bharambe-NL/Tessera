/**
 * Doc 09 section 14 and doc 11 section 10, measured rather than asserted.
 *
 * "Every verb reachable by keyboard; flag rows navigable with arrows; card focus
 * ring; reduced motion respected; text contrast at 4.5:1" and "canvas has a list
 * view alternative (the board's cards as a document) for screen readers".
 *
 * Contrast is computed from what the browser actually painted, not from the
 * token palette. The palette is OKLCH and the ratio is defined on sRGB
 * luminance, so a number read off the tokens would be a conversion this file
 * got right or wrong with nothing to check it against. Reading `getComputedStyle`
 * asks the renderer instead.
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

async function askOne(page: import('@playwright/test').Page): Promise<void> {
  await page.locator('#ask').fill(QUESTION);
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer')).toBeVisible({ timeout: 30_000 });
}

/**
 * Every text-bearing element on screen, with the contrast ratio between its
 * text and the first opaque background behind it.
 *
 * Returns only the failures, each with enough to find it, so a failure message
 * names the element rather than a count.
 */
async function contrastFailures(page: import('@playwright/test').Page) {
  return await page.evaluate(() => {
    const srgb = (c: number) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
    const parse = (value: string): [number, number, number, number] | null => {
      const m = value.match(/rgba?\(([^)]+)\)/);
      if (!m) return null;
      const parts = m[1].split(/[,\s/]+/).filter(Boolean).map(Number);
      const [r, g, b, a] = parts;
      return [r, g, b, a === undefined ? 1 : a];
    };
    const luminance = ([r, g, b]: [number, number, number, number]) =>
      0.2126 * srgb(r / 255) + 0.7152 * srgb(g / 255) + 0.0722 * srgb(b / 255);

    const behind = (el: Element): [number, number, number, number] => {
      let node: Element | null = el;
      while (node) {
        const bg = parse(getComputedStyle(node).backgroundColor);
        if (bg && bg[3] > 0.95) return bg;
        node = node.parentElement;
      }
      return [255, 255, 255, 1];
    };

    const out: { text: string; selector: string; ratio: number; needed: number }[] = [];
    for (const el of Array.from(document.querySelectorAll<HTMLElement>('body *'))) {
      // Only elements that paint their own text.
      const own = Array.from(el.childNodes).some(
        (n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim().length > 1,
      );
      if (!own) continue;
      const style = getComputedStyle(el);
      if (style.visibility === 'hidden' || style.display === 'none') continue;
      const box = el.getBoundingClientRect();
      if (box.width === 0 || box.height === 0) continue;
      if (Number(style.opacity) < 0.5) continue;

      const fg = parse(style.color);
      if (!fg || fg[3] < 0.5) continue;
      const bg = behind(el);
      const [lo, hi] = [luminance(fg), luminance(bg)].sort((a, b) => a - b);
      const ratio = (hi + 0.05) / (lo + 0.05);

      // Doc 11 section 10: 4.5:1 for text, 3:1 for large. Large is 18pt, or
      // 14pt bold, which is 24px and 18.66px in css pixels.
      const size = parseFloat(style.fontSize);
      const bold = Number(style.fontWeight) >= 700 || style.fontWeight === 'bold';
      const large = size >= 24 || (bold && size >= 18.66);
      const needed = large ? 3 : 4.5;

      if (ratio + 0.01 < needed) {
        out.push({
          text: (el.textContent ?? '').trim().slice(0, 40),
          selector: `${el.tagName.toLowerCase()}.${el.className || '(none)'}`,
          ratio: Math.round(ratio * 100) / 100,
          needed,
        });
      }
    }
    return out;
  });
}

test('text on the board meets the contrast floor', async ({ page }) => {
  await askOne(page);
  const failures = await contrastFailures(page);
  expect(failures, JSON.stringify(failures, null, 2)).toEqual([]);
});

test('text on every page meets the contrast floor', async ({ page }) => {
  await askOne(page);
  for (const view of ['home', 'flags', 'library', 'profile'] as const) {
    await page.locator(`#rail [data-view="${view}"]`).click();
    await expect(page.locator('#page-title')).not.toBeEmpty();
    const failures = await contrastFailures(page);
    expect(failures, `${view}: ${JSON.stringify(failures, null, 2)}`).toEqual([]);
  }
});

test('every verb on a card is reachable by keyboard', async ({ page }) => {
  await askOne(page);

  // Tab from the top of the document and collect what receives focus. Doc 09
  // section 14: every verb reachable by keyboard, so every one of these has to
  // appear without a pointer touching the page.
  const reached = new Set<string>();
  await page.locator('body').press('Tab');
  for (let i = 0; i < 40; i += 1) {
    const id = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el || el === document.body) return null;
      // `id` and `className` are empty strings rather than undefined on an
      // element that has neither, so `??` never falls through them.
      return el.dataset.act || el.id || el.className || el.tagName.toLowerCase();
    });
    if (id) reached.add(id);
    await page.keyboard.press('Tab');
  }

  // The card's own verbs: the flag chip, rerun, the follow-up box and its send.
  expect([...reached]).toEqual(expect.arrayContaining(['flags', 'rerun', 'follow']));
  expect([...reached].some((r) => r.includes('followup'))).toBe(true);
  // And the shell: the rail, the title, the composer.
  expect([...reached].some((r) => r.includes('rail-item'))).toBe(true);
  expect(reached.has('title')).toBe(true);
  expect(reached.has('ask')).toBe(true);
});

test('a focused card verb shows a focus ring', async ({ page }) => {
  await askOne(page);
  const rerun = page.locator('#cards .card [data-act="rerun"]');
  await rerun.focus();

  const outline = await rerun.evaluate((el) => {
    const s = getComputedStyle(el);
    return { width: s.outlineWidth, style: s.outlineStyle };
  });
  expect(outline.style).not.toBe('none');
  expect(parseFloat(outline.width)).toBeGreaterThan(0);
});

test('flag rows are navigable with arrows', async ({ page }) => {
  await askOne(page);
  await page.locator('#rail [data-view="home"]').click();
  await page.locator('#home-create').click();
  await expect(page.locator('#page')).toBeHidden();
  await askOne(page);

  await page.locator('#rail [data-view="flags"]').click();
  await expect(page.locator('.flag-row')).toHaveCount(2);

  // Doc 09 section 14: "flag rows navigable with arrows".
  await page.locator('.flag-row').first().focus();
  await expect(page.locator('.flag-row').first()).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('.flag-row').nth(1)).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(page.locator('.flag-row').first()).toBeFocused();
  // Off the end, focus stays rather than wrapping into nothing.
  await page.keyboard.press('ArrowUp');
  await expect(page.locator('.flag-row').first()).toBeFocused();
});

test('the board reads as a document', async ({ page }) => {
  await askOne(page);
  // Branch, so the document has a relation to state that the canvas draws as an
  // edge and a screen reader cannot see.
  const card = page.locator('#cards .card').first();
  await card.locator('.followup').fill('which article says so?');
  await card.locator('.followup').press('Enter');
  await expect(page.locator('#cards .card')).toHaveCount(2, { timeout: 30_000 });

  await page.locator('#reading-toggle').click();
  const reading = page.locator('#reading');
  await expect(reading).toBeVisible();
  await expect(page.locator('#reading-toggle')).toHaveAttribute('aria-pressed', 'true');

  // Real headings, in reading order, parents before children.
  const headings = await reading.locator('h3').allTextContents();
  expect(headings).toHaveLength(2);
  expect(headings[0]).toContain(QUESTION);
  expect(headings[1]).toContain('which article says so?');

  // The edge the canvas draws is stated instead.
  await expect(reading.locator('section').nth(1).locator('.meta')).toContainText('follow-up');
  // And the visual is described rather than drawn.
  await expect(reading).toContainText('The visual is a tree');

  // Two copies of every card in the accessibility tree is worse than either
  // one, so the canvas is hidden from it while the document is open.
  await expect(page.locator('#world')).toHaveAttribute('aria-hidden', 'true');

  await page.locator('#reading-toggle').click();
  await expect(reading).toBeHidden();
  await expect(page.locator('#world')).toHaveAttribute('aria-hidden', 'false');
});

test('reduced motion is respected', async ({ browser }) => {
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  const page = await context.newPage();
  await freshCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await askOne(page);

  // Doc 11 section 7: the rise animation and the rail transition both stand
  // down. A card that animates in under reduce is the one motion a reader who
  // asked for none cannot look away from.
  const animation = await page
    .locator('#cards .card')
    .first()
    .evaluate((el) => getComputedStyle(el).animationName);
  expect(animation).toBe('none');

  const railTransition = await page
    .locator('#rail')
    .evaluate((el) => getComputedStyle(el).transitionDuration);
  expect(parseFloat(railTransition)).toBe(0);

  await context.close();
});
