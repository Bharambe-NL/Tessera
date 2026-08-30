/**
 * Doc 11 section 5: "composer bottom centre".
 *
 * Centre of what was the question nobody had asked. `left: 50%` measures the
 * window, and the canvas begins where the rail ends, so the composer sat half a
 * rail to the left of the space it belongs in. With the rail open that is 120px
 * and its left edge landed under the rail, which drew on top of it.
 *
 * The second half is the grid. Four children were laid into three columns, so
 * the send button wrapped to a row of its own underneath the question, and the
 * three that did fit were aligned at their bottoms alone.
 */

import { expect, test, type Page } from '@playwright/test';

import { freshCore, useCore } from './shell.js';

test.beforeEach(async ({ page }) => {
  await freshCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
});

/** The rail's width, which the composer has to agree with. */
async function railWidth(page: Page): Promise<number> {
  return await page.evaluate(() =>
    parseFloat(getComputedStyle(document.body).getPropertyValue('--rail-w')),
  );
}

/**
 * Wait until the composer has finished travelling.
 *
 * The composer slides on the rail's own 360ms, so a measurement taken straight
 * after the toggle reads a point on the way rather than the destination.
 * Comparing `left` against the rail's width waits for the thing that has to be
 * true instead of for a duration.
 */
async function composerSettled(page: Page): Promise<void> {
  await expect(async () => {
    const [left, rail] = await Promise.all([
      page.evaluate(() => getComputedStyle(document.getElementById('composer')!).left),
      railWidth(page),
    ]);
    expect(parseFloat(left)).toBeCloseTo(rail, 0);
  }).toPass({ timeout: 5_000 });
}

/** How far the composer's centre sits from the centre of the visible canvas. */
async function offCentreBy(page: Page): Promise<number> {
  return await page.evaluate(() => {
    const r = document.getElementById('composer')!.getBoundingClientRect();
    const rail = parseFloat(getComputedStyle(document.body).getPropertyValue('--rail-w'));
    return Math.round(rail + (window.innerWidth - rail) / 2) - Math.round(r.left + r.width / 2);
  });
}

test('the composer centres on the canvas, not on the window', async ({ page }) => {
  await composerSettled(page);
  expect(Math.abs(await offCentreBy(page))).toBeLessThanOrEqual(1);
});

test('an open rail moves the composer instead of covering it', async ({ page }) => {
  await composerSettled(page);
  await page.locator('#rail-toggle').click();
  await expect(page.locator('body')).toHaveClass(/rail-open/);
  await composerSettled(page);

  expect(Math.abs(await offCentreBy(page))).toBeLessThanOrEqual(1);

  // The complaint underneath the complaint: at 240px the rail was drawn over
  // the composer's left edge, so the question box began underneath it.
  const clear = await page.evaluate(() => {
    const r = document.getElementById('composer')!.getBoundingClientRect();
    const rail = parseFloat(getComputedStyle(document.body).getPropertyValue('--rail-w'));
    return r.left >= rail;
  });
  expect(clear).toBe(true);
});

test('the model control offers the models whose keys exist, and the pick rides the ask', async ({ page }) => {
  // Owner decision 2026-08-30: the model is the user's to pick from the chat
  // window. The control fills from the profile after boot: Auto first, then one
  // option per distinct model whose key is in the keychain. The dev core holds
  // one Anthropic shaped key, so the Claude tiers are offered and no Kimi tier
  // is.
  const model = page.locator('#model');
  await expect(model).toBeVisible();
  await expect(model.locator('option')).toHaveCount(4);
  const labels = await model.locator('option').allTextContents();
  expect(labels[0]).toBe('Auto');
  expect(labels).toContain('claude-opus-5');
  expect(labels).toContain('claude-sonnet-5');
  expect(labels).not.toContain('kimi-k3');

  // The chosen alias rides `card.ask` as `model`, which the core pins the
  // answer stage to. Read off the wire rather than off the state, because the
  // wire is the contract.
  const asked: unknown[] = [];
  await page.route('**/rpc', async (route) => {
    const body = route.request().postDataJSON() as
      | { method?: string; params?: { model?: unknown } }
      | null;
    if (body?.method === 'card.ask') asked.push(body.params?.model);
    await route.continue();
  });
  await model.selectOption({ label: 'claude-sonnet-5' });
  await page.locator('#ask').fill('What is a world model?');
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer')).toBeVisible({ timeout: 30_000 });
  expect(asked).toEqual(['medium']);
});

test('the question takes a row and the controls take the one below', async ({ page }) => {
  const rows = await page.evaluate(() => {
    const box = (id: string) => document.getElementById(id)!.getBoundingClientRect();
    const centre = (id: string) => Math.round(box(id).top + box(id).height / 2);
    return {
      askCentre: centre('ask'),
      learn: centre('learn'),
      modes: centre('modes'),
      send: centre('send'),
      sendRight: Math.round(box('send').right),
      modesRight: Math.round(box('modes').right),
    };
  });

  // Send wrapped to an implicit third row and landed back in the first column,
  // underneath the question, because the grid declared three columns for four
  // children.
  expect(rows.send).toBeGreaterThan(rows.askCentre);
  expect(rows.sendRight).toBeGreaterThan(rows.modesRight);

  // A 19px Learn button between a 32px field and a 26px group of pills looked
  // like it was hanging, because only their bottoms were ever aligned.
  expect(rows.learn).toBe(rows.modes);
  expect(rows.learn).toBe(rows.send);
});
