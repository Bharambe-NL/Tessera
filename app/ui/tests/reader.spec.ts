/**
 * Pasting an image and reading it. Doc 07 part A.
 *
 * The deterministic half of the Reader is asserted in Rust. What this covers is
 * the gesture: a person pastes a screenshot of a table onto a board and gets a
 * card about it, with the header doc 07 section A11 asks for.
 */

import { expect, test } from '@playwright/test';

import { freshCore, useCore } from './shell.js';

/**
 * An 8x8 greyscale png with a dark diagonal, generated rather than remembered.
 *
 * The first version of this was a base64 string that looked like a png and was
 * not one: its chunk table walked off the end. `createImageBitmap` refused it,
 * the page reported that the image could not be read, and the failure looked
 * like a bug in the paste path for two rounds.
 */
const PNG_8X8 =
  'iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAAAAADhZOFXAAAAJUlEQVR42mMQuQMBDHdEYAwo' +
  'C8iAsEAMMAvMALEgDCALyrgjAgDbojDBcf3cJAAAAABJRU5ErkJggg==';

test.beforeEach(async ({ page }) => {
  await freshCore(page);
  await useCore(page);
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
});

/**
 * Paste the way a browser delivers a clipboard.
 *
 * `alsoText` puts a text flavour on it too, which is what copying from a
 * document gives you and what separates "read this picture" from "type this
 * word".
 */
async function paste(
  page: import('@playwright/test').Page,
  base64: string,
  alsoText = false,
): Promise<void> {
  await page.evaluate(
    async ({ data, withText }) => {
      const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
      const file = new File([bytes], 'pasted.png', { type: 'image/png' });
      const transfer = new DataTransfer();
      transfer.items.add(file);
      if (withText) transfer.items.add('some copied words', 'text/plain');
      document.dispatchEvent(
        new ClipboardEvent('paste', { clipboardData: transfer, bubbles: true, cancelable: true }),
      );
    },
    { data: base64, withText: alsoText },
  );
}

test('a pasted image becomes a read card', async ({ page }) => {
  await paste(page, PNG_8X8);

  await expect(page.locator('#cards .card')).toHaveCount(1, { timeout: 30_000 });
  const card = page.locator('#cards .card').first();
  await expect(card).not.toHaveAttribute('data-status', 'failed');

  // Doc 07 section A11: a read card says where it came from, and it does not
  // show a question bubble, because nobody typed the question.
  await expect(card.locator('.head .title')).toHaveText('Read from an image');
  await expect(card.locator('.msg')).toHaveCount(0);

  // The description is the card's answer.
  await expect(card.locator('.answer')).toContainText('table');
});

test('a paste that carries text into the composer stays text', async ({ page }) => {
  // Someone pasting a question they copied should get their question, not a
  // card about the screenshot that happened to be on the clipboard too.
  await page.locator('#ask').focus();
  await paste(page, PNG_8X8, true);
  await page.waitForTimeout(1000);
  await expect(page.locator('#cards .card')).toHaveCount(0);
});

test('an image paste is read even though the composer has focus', async ({ page }) => {
  // `boot` focuses the composer, so the composer always has focus. A rule that
  // checked only the focus would have blocked every image paste there is.
  await page.locator('#ask').focus();
  await paste(page, PNG_8X8);
  await expect(page.locator('#cards .card')).toHaveCount(1, { timeout: 30_000 });
});
