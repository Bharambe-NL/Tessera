/**
 * First run, driven end to end. Doc 11 section 6 and doc 12 phase 11.
 *
 * The acceptance doc 12 phase 11 names is a fresh install to a first verified
 * deep card, so what these cover is the part between the two: that a person
 * with an empty keychain is shown the setup, that the setup does what it says,
 * and that finishing it leaves them somewhere they can ask a question.
 */

import { expect, test } from '@playwright/test';

import { freshCore, keylessCore, useCore } from './shell.js';

test('a profile with no key opens on the setup screen', async ({ page }) => {
  await keylessCore(page);
  await useCore(page);
  await page.goto('/');

  await expect(page.locator('#page')).toBeVisible();
  await expect(page.locator('#page-title')).toHaveText('Set up Tessera');
  await expect(page.locator('ol.setup .step')).toHaveCount(3);

  // Doc 11 section 6: choose a pack, add a key, optionally a folder. The pack
  // step is already satisfied because a profile always has one.
  await expect(page.locator('[data-step="1"]')).toHaveClass(/done/);
  await expect(page.locator('[data-step="2"]')).not.toHaveClass(/done/);

  // And the way out is closed until the key is in, with the reason said rather
  // than left for the person to work out from a dead button.
  await expect(page.locator('#setup-done')).toBeDisabled();
  await expect(page.locator('.setup-acts .note')).toContainText('model key');
});

test('a profile with a key does not see the setup at all', async ({ page }) => {
  // The state everyone else is in. A setup screen shown to someone who has
  // already finished is the failure this guards.
  await freshCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await expect(page.locator('#page')).toBeHidden();
  await expect(page.locator('#composer')).toBeVisible();
});

test('saving a key finishes the step and opens the way out', async ({ page }) => {
  await keylessCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#setup-secret')).toBeVisible();

  await page.locator('#setup-secret').fill('sk-a-key-for-this-test');
  await page.locator('#setup-key button[type="submit"]').click();

  await expect(page.locator('[data-step="2"]')).toHaveClass(/done/, { timeout: 30_000 });
  await expect(page.locator('#setup-done')).toBeEnabled();
  // Doc 10 section 8: the secret goes to the keychain and nothing sends it
  // back. The screen can only say that the keychain has one.
  await expect(page.locator('[data-step="2"]')).toContainText('keychain');
  await expect(page.locator('[data-step="2"]')).not.toContainText('sk-a-key');
});

test('the key never stays in the field it was typed into', async ({ page }) => {
  // The field holds the only copy of the secret in the page, and the screen
  // stays up on a failure, so it is cleared before the call rather than after.
  await keylessCore(page);
  await useCore(page);
  await page.goto('/');
  await page.locator('#setup-secret').fill('sk-a-key-for-this-test');
  await page.locator('#setup-key button[type="submit"]').click();
  await expect(page.locator('[data-step="2"]')).toHaveClass(/done/, { timeout: 30_000 });

  const html = await page.locator('#page').innerHTML();
  expect(html).not.toContain('sk-a-key-for-this-test');
});

test('finishing setup lands on a board that can be asked a question', async ({ page }) => {
  // Doc 12 phase 11's acceptance, up to the point where money would be spent.
  await keylessCore(page);
  await useCore(page);
  await page.goto('/');
  await page.locator('#setup-secret').fill('sk-a-key-for-this-test');
  await page.locator('#setup-key button[type="submit"]').click();
  await expect(page.locator('#setup-done')).toBeEnabled({ timeout: 30_000 });

  await page.locator('#setup-done').click();
  await expect(page.locator('#page')).toBeHidden();
  await expect(page.locator('#composer')).toBeVisible();

  await page.locator('#ask').fill('what are world models?');
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card .answer')).toHaveCount(1, { timeout: 60_000 });
});

test('a folder that does not exist is refused with a reason', async ({ page }) => {
  // A path typed with a typo is the common case, and a setup that accepted it
  // would leave a retriever pointed at nothing and say everything went well.
  await keylessCore(page);
  await useCore(page);
  await page.goto('/');

  await page.locator('#setup-folder-root').fill('/no/such/folder/anywhere');
  await page.locator('#setup-folder-label').fill('Nothing');
  await page.locator('#setup-folder button[type="submit"]').click();

  await expect(page.locator('.setup-error')).toContainText('does not exist', { timeout: 30_000 });
  await expect(page.locator('[data-step="3"]')).not.toHaveClass(/done/);
});

test('choosing a pack switches it', async ({ page }) => {
  await keylessCore(page);
  await useCore(page);
  await page.goto('/');

  const finance = page.locator('[data-setup-pack="finance-eu"]');
  await expect(finance).toBeVisible();
  await finance.click();
  await expect(finance).toHaveClass(/on/, { timeout: 30_000 });
  // And it is the core that says so, not the button remembering its own click.
  await page.reload();
  await expect(page.locator('[data-setup-pack="finance-eu"]')).toHaveClass(/on/);
});
