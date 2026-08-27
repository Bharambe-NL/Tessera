/**
 * The Map. Doc 17 section 6 and phase 13d.
 *
 * The layout rule and the state colours are asserted in TypeScript unit terms
 * nowhere, because both are functions of what the core returned: the depth and
 * the frontier are the core's answer and this view draws them. What these tests
 * cover is the part only a browser can say, which is that a learner can find
 * the map, read what they know from it, open a node, change a rating and leave
 * for a lesson.
 */

import { expect, test } from '@playwright/test';

import { learningCore, useCore } from './shell.js';

test.beforeEach(async ({ page }) => {
  await learningCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await page.locator('.rail-item[data-view="map"]').click();
  await expect(page.locator('#page-title')).toHaveText('Map');
});

test('the map draws a node per concept, layered by what they depend on', async ({ page }) => {
  const nodes = page.locator('.map-node');
  await expect(nodes).toHaveCount(5, { timeout: 30_000 });

  // Doc 17 section 6: "layered by prerequisite depth, never hand arranged". The
  // fixture is a chain of five, so every node sits in a band of its own and the
  // one with no prerequisite is at the top.
  const ys = await page.locator('.map-node circle').evaluateAll((circles) =>
    circles.map((c) => Number(c.getAttribute('cy'))),
  );
  expect(new Set(ys).size).toBe(5);

  const first = await page
    .locator('.map-node')
    .filter({ hasText: 'state space' })
    .locator('circle')
    .getAttribute('cy');
  expect(Math.min(...ys)).toBe(Number(first));
});

test('a confirmed prerequisite is solid and a proposed one is dotted', async ({ page }) => {
  // Doc 17 section 7: agents propose, the learner confirms, and the map has to
  // show which is which or a guess reads as an agreement.
  await expect(page.locator('.map-edge.confirmed')).toHaveCount(4, { timeout: 30_000 });
  await expect(page.locator('.map-edge.proposed')).toHaveCount(1);
});

test('the frontier is a band, and it sits at the lowest thing nobody has checked', async ({
  page,
}) => {
  // Doc 17 section 3: "the lowest prerequisite level where rated concepts have
  // a rating of 2 or more and mastery is still unverified". The fixture rates
  // the first three, none of them has been checked, so the frontier is the
  // shallowest of the three rather than the deepest. A learner who has only
  // ever claimed is put at the bottom of what they claimed, which is the rule
  // that catches the overconfident rater.
  await expect(page.locator('.map-band')).toHaveCount(1, { timeout: 30_000 });
  const onFrontier = page.locator('.map-node.frontier');
  await expect(onFrontier).toHaveCount(1);
  await expect(onFrontier).toContainText('state space');
});

test('a state filter shows only the concepts in that state', async ({ page }) => {
  await expect(page.locator('.map-node')).toHaveCount(5, { timeout: 30_000 });
  await page.locator('[data-map-filter="rated"]').click();
  await expect(page.locator('.map-node')).toHaveCount(3);
  await page.locator('[data-map-filter="mastered"]').click();
  await expect(page.locator('.page-empty')).toContainText('No concept matches');
  await page.locator('[data-map-filter="all"]').click();
  await expect(page.locator('.map-node')).toHaveCount(5);
});

test('opening a node shows what is known about it and takes a new rating', async ({ page }) => {
  await page.locator('.map-node').filter({ hasText: 'world model' }).click();
  const panel = page.locator('.map-panel');
  await expect(panel.locator('h3')).toHaveText('world model');

  // Doc 17 sections 2.1 and 2.4: a rating sets a starting prior, so a number
  // exists, and it is a claim rather than a measurement. The panel says which,
  // because a claim shown as a score is the learner's own guess handed back to
  // them as evidence.
  await expect(panel).toContainText('From what you said, that is 35%');
  await expect(panel).toContainText('No check has confirmed it.');

  // Doc 17 section 2.1: the rating already claimed is the one shown as chosen.
  await expect(panel.locator('[data-map-rate="2"]')).toHaveClass(/on/);
  await page.locator('[data-map-rate="3"]').click();
  await expect(panel.locator('[data-map-rate="3"]')).toHaveClass(/on/, { timeout: 30_000 });

  await page.locator('[data-map-act="close"]').click();
  await expect(page.locator('.map-node')).toHaveCount(5);
});

test('a node opens a lesson on the concept it names', async ({ page }) => {
  await page.locator('.map-node').filter({ hasText: 'world model' }).click();
  await expect(page.locator('.map-panel')).toBeVisible();
  await page.locator('[data-map-act="lesson"]').click();

  // Doc 17 section 6: the verb lands the learner on a board with a session, so
  // the page layer is gone and the tutor is there.
  await expect(page.locator('#tutor')).toBeVisible({ timeout: 60_000 });
});
