/**
 * A Learn session, driven end to end. Doc 14.
 *
 * The four rules of doc 14 section 3.5 are asserted in Rust, where they are
 * deterministic checks over a turn. What this covers is the part Rust cannot:
 * that a learner can name a topic, answer the intake, get a plan, watch it turn
 * into cards, be checked on one, and end the session with a score.
 *
 * Every assertion below names a screen the learner reaches, because a Learn
 * session that renders and a Learn session that advances are different claims,
 * and the first version of this surface rendered intake and then stopped there.
 */

import { expect, test, type Page } from '@playwright/test';

import { freshCore, useCore } from './shell.js';

test.beforeEach(async ({ page }) => {
  await freshCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
});

/** Turn Learn on and name a topic, which is the whole of doc 14 section 4. */
async function startLearning(page: Page, topic: string): Promise<void> {
  await page.locator('#learn').click();
  await expect(page.locator('#ask')).toHaveAttribute('placeholder', 'What do you want to learn?');
  await page.locator('#ask').fill(topic);
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#tutor')).toBeVisible();
}

/**
 * Answer every intake question the tutor asked, first option each time.
 *
 * The loop waits on the count of unanswered questions rather than on the
 * button it just clicked. A `.first()` locator re-resolves on every poll, so
 * once the first question is gone it resolves to the next question's first
 * option and reports it visible forever: the first version of this waited
 * thirty seconds for a button that had already done its job.
 */
async function answerIntake(page: Page): Promise<void> {
  const asks = page.locator('#tutor-body .ask[data-q]');
  await expect(asks.first()).toBeVisible({ timeout: 30_000 });
  for (let remaining = await asks.count(); remaining > 0; remaining--) {
    await asks.first().locator('button').first().click();
    await expect(asks).toHaveCount(remaining - 1, { timeout: 30_000 });
  }
}

test('a session runs from a topic to a plan to a checked card', async ({ page }) => {
  await startLearning(page, 'world models');

  // Doc 14 section 3.9: the stage label says where the session is, in words.
  await expect(page.locator('#tutor-stage')).toHaveText('Getting to know you');
  await expect(page.locator('#tutor-body')).toContainText('Learning about world models');

  await answerIntake(page);

  // Intake done means a plan, without the learner asking for one.
  await expect(page.locator('#tutor-body .plan')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#tutor-stage')).toHaveText('Planning the board');
  const planned = page.locator('#tutor-body .plan ol li');
  await expect(planned).toHaveCount(3);

  // Doc 14 section 3.4: the cards are ordinary cards through the ordinary
  // pipeline, so they land on the board the learner was already looking at.
  await page.locator('#learn-open-plan').click();
  await expect(page.locator('#cards .card')).toHaveCount(3, { timeout: 90_000 });
  await expect(page.locator('#cards .card .answer')).toHaveCount(3);

  // Building rolls straight into a check on what was just built.
  const check = page.locator('#tutor-body .ask').last();
  await expect(check).toBeVisible({ timeout: 60_000 });
  await expect(page.locator('#tutor-stage')).toHaveText('Checking understanding');
  // Nothing is marked before the learner answers; a mark would be the answer.
  await expect(page.locator('#tutor-body .opt.right')).toHaveCount(0);

  await check.locator('.opt').first().click();
  await expect(page.locator('#tutor-body .feedback')).toHaveClass(/right/, { timeout: 30_000 });
  await expect(page.locator('#tutor-body .feedback')).toContainText('Right.');

  // Doc 14 section 3.4: then a choice, never an automatic next step.
  await expect(page.locator('[data-learn-act="next"]')).toBeVisible();
  await expect(page.locator('[data-learn-act="another"]')).toBeVisible();
  await expect(page.locator('[data-learn-act="stop"]')).toBeVisible();
});

test('a wrong answer is told it is wrong and still explained', async ({ page }) => {
  await startLearning(page, 'world models');
  await answerIntake(page);
  await page.locator('#learn-open-plan').click();

  const check = page.locator('#tutor-body .ask').last();
  await expect(check.locator('.opt')).toHaveCount(3, { timeout: 90_000 });
  await check.locator('.opt').nth(1).click();

  const feedback = page.locator('#tutor-body .feedback');
  await expect(feedback).toHaveClass(/wrong/, { timeout: 30_000 });
  await expect(feedback).toContainText('Not quite.');
  // Doc 14 section 3.6: a wrong answer gets the explanation, not just the mark.
  await expect(feedback).toContainText('The card opens with it.');
  // And the right option is shown once it can no longer be the answer.
  await expect(page.locator('#tutor-body .opt.right')).toHaveCount(1);
});

test('the next card opens as a follow-up on the card that was checked', async ({ page }) => {
  await startLearning(page, 'world models');
  await answerIntake(page);
  await page.locator('#learn-open-plan').click();

  const check = page.locator('#tutor-body .ask').last();
  await expect(check.locator('.opt')).toHaveCount(3, { timeout: 90_000 });
  await check.locator('.opt').first().click();
  await expect(page.locator('#tutor-body .feedback')).toBeVisible({ timeout: 30_000 });

  await page.locator('[data-learn-act="next"]').click();
  await expect(page.locator('#cards .card')).toHaveCount(4, { timeout: 90_000 });
});

test('ending the session reports the score and leaves the board standing', async ({ page }) => {
  await startLearning(page, 'world models');
  await answerIntake(page);
  await page.locator('#learn-open-plan').click();

  const check = page.locator('#tutor-body .ask').last();
  await expect(check.locator('.opt')).toHaveCount(3, { timeout: 90_000 });
  await check.locator('.opt').first().click();
  await expect(page.locator('#tutor-body .feedback')).toBeVisible({ timeout: 30_000 });

  await page.locator('[data-learn-act="stop"]').click();
  await expect(page.locator('#toasts')).toContainText('Session over.', { timeout: 30_000 });
  await expect(page.locator('#tutor')).toBeHidden();
  await expect(page.locator('#learn')).toHaveAttribute('aria-pressed', 'false');
  // Doc 14 section 3.9: the board keeps everything the session built.
  await expect(page.locator('#cards .card')).toHaveCount(3);
});

test('closing the panel ends the session', async ({ page }) => {
  await startLearning(page, 'world models');
  await expect(page.locator('#tutor-body .ask').first()).toBeVisible({ timeout: 30_000 });

  await page.locator('#tutor-close').click();
  await expect(page.locator('#tutor')).toBeHidden();
  await expect(page.locator('#ask')).toHaveAttribute('placeholder', 'Ask something');

  // And the composer is back to asking questions rather than naming topics.
  await page.locator('#ask').fill('what are world models?');
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer')).toHaveCount(1, { timeout: 60_000 });
});

test('a learner can skip intake and get the plan anyway', async ({ page }) => {
  // Doc 14 section 3.4: "the learner may skip intake with just build it".
  await startLearning(page, 'world models');
  await expect(page.locator('#tutor-body .ask').first()).toBeVisible({ timeout: 30_000 });

  await page.locator('[data-learn-act="build"]').click();
  await expect(page.locator('#tutor-body .plan')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#tutor-body .ask [data-intake]')).toHaveCount(0);
});

test('asking the tutor a question gets an answer with no citation marker in it', async ({
  page,
}) => {
  // Doc 14 section 3.5 rule 4: the tutor speaks in its own words and never
  // shows the citation markers the cards carry. Asserted in Rust over a turn;
  // asserted here over what the learner actually reads.
  await startLearning(page, 'world models');
  await answerIntake(page);
  await page.locator('#learn-open-plan').click();
  await expect(page.locator('#tutor-body .ask').last().locator('.opt')).toHaveCount(3, {
    timeout: 90_000,
  });

  await page.locator('#learn-message').fill('so what does a world model actually do?');
  await page.locator('#learn-message').press('Enter');

  const said = page.locator('#tutor-body .say.tutor').last();
  await expect(said).toContainText('world model', { timeout: 30_000 });
  expect(await said.innerText()).not.toMatch(/\[\d+\]/);
});
