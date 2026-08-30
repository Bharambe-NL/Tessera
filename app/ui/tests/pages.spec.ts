/**
 * The rail and the four pages. Doc 11 sections 5 and 6.
 *
 * These drive the real product against a real core, so a page that renders from
 * a read the core does not serve fails here rather than at first use. The
 * lesson from step 1 holds: counting rows is not enough, so where a verb writes
 * something these assert what the write did.
 */

import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, test } from '@playwright/test';

import { freshCore, useCore } from './shell.js';

/** The repository root, so a test can read the packs the app ships. */
const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

const QUESTION = 'what are world models?';

/**
 * A pack file a person could plausibly have: the general pack under a code and
 * version of their own, so it validates for the reasons the shipped one does.
 */
function packFile(code: string, version: string): string {
  const pack = JSON.parse(readFileSync(join(REPO, 'packs', 'general.json'), 'utf8')) as Record<
    string,
    unknown
  >;
  pack.code = code;
  pack.name = 'A pack of my own';
  pack.version = version;
  const dir = mkdtempSync(join(tmpdir(), 'tessera-pack-'));
  const file = join(dir, `${code}-${version}.json`);
  writeFileSync(file, JSON.stringify(pack, null, 2));
  return file;
}

async function importPack(page: import('@playwright/test').Page, file: string): Promise<void> {
  await page.locator('#rail [data-view="profile"]').click();
  await page.locator('#pack-path').fill(file);
  // The import is a fetch the click starts, and the caller's next move may be a
  // reload that tears the page down. A navigation aborts an in flight request
  // the server has not read yet, so the pack this test wrote would never land:
  // the response arriving is the core saying it has it. Armed before the click,
  // because a response cannot be waited for after it has been received.
  const landed = page.waitForResponse(
    (r) => r.request().postData()?.includes('"pack.import"') ?? false,
    { timeout: 30_000 },
  );
  await page.locator('#pack-import button[type="submit"]').click();
  await landed;
}

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

test('library shows both tabs, and a concept is proposed then confirmed', async ({ page }) => {
  await askOne(page);
  await page.locator('#rail [data-view="library"]').click();

  // A fast card retrieves nothing, so Sources is honestly empty and says what
  // would put something in it.
  await expect(page.locator('.page-empty')).toContainText('No sources yet');

  // Concepts is not empty, because the Router named an entity and doc 01
  // section 4.10's write path now turns that into a proposal. Before M9 step 4
  // this tab could never have had a row: nothing wrote a Concept.
  await page.locator('[data-library-tab="concepts"]').click();
  const row = page.locator('.lib-row[data-concept]');
  await expect(row).toHaveCount(1);
  await expect(row.locator('.title')).toHaveText('world model');
  await expect(row.locator('.chip.status-proposed')).toHaveText('proposed');

  // "Agents propose; the user confirms." Confirming is the user's half.
  await row.locator('[data-concept-act="accept"]').click();
  await expect(page.locator('.chip.status-confirmed')).toHaveText('confirmed');
  // A confirmed concept has no decision left to make, so it offers none.
  await expect(page.locator('[data-concept-act="accept"]')).toHaveCount(0);

  await page.locator('[data-library-tab="sources"]').click();
  await expect(page.locator('[data-library-tab="sources"]')).toHaveClass(/on/);
});

test('a board offers the pack update that judges its cards again', async ({ page }) => {
  // Doc 10 section 9: "a pack update never rewrites a board's pinned version;
  // the board offers update pack, which reruns verify_only". So importing a
  // newer version changes nothing until the board is asked, and then it says
  // what it did.
  await importPack(page, packFile('house-rules', '0.1.0'));
  await page.locator('.lib-row[data-pack="house-rules"] [data-pack-act="use"]').click();
  await expect(page.locator('.lib-row[data-pack="house-rules"]')).toContainText('Active', {
    timeout: 30_000,
  });

  // A board created after the switch pins the pack that was active then.
  await page.locator('#rail [data-view="home"]').click();
  await page.locator('#home-create').click();
  await askOne(page);
  await expect(page.locator('#pack-update')).toBeHidden();

  // The pack's author ships a new version.
  await importPack(page, packFile('house-rules', '0.2.0'));
  await page.reload();

  const update = page.locator('#pack-update');
  await expect(update).toBeVisible({ timeout: 30_000 });
  await expect(update).toContainText('0.1.0');
  await expect(update).toContainText('0.2.0');

  await update.click();
  await expect(page.locator('#toasts')).toContainText('Judged again under', { timeout: 30_000 });
  // The board is on the new version now, so there is nothing left to offer.
  await expect(update).toBeHidden({ timeout: 30_000 });
});

test('the profile is one page whose sections all show, and never a key', async ({ page }) => {
  // Owner decision 2026-08-30: no tabs. The five sections of doc 11 section 6
  // sit on one scrolling page, so everything below asserts without a click.
  await page.locator('#rail [data-view="profile"]').click();
  await expect(page.locator('[data-profile-section]')).toHaveCount(5);
  await expect(page.locator('[data-profile-section="context"] .facts')).toBeVisible();

  const rows = page.locator('.lib-row[data-key-ref]');
  await expect(rows.first()).toBeVisible();

  // Doc 10 section 8 and the standing rule. The dev server holds a fake key
  // whose value is known, so this is a real check that it never crossed.
  const html = await page.locator('#page-body').innerHTML();
  expect(html).not.toContain('sk-');
  await expect(page.locator('[data-profile-section="models"] .page-note')).toContainText('keychain');

  await expect(page.locator('[data-profile-section="diagnostics"] .facts')).toContainText('Events');
  await expect(page.locator('.lib-row[data-pack="general"]')).toContainText('Ships with the app');
});

test('a doctrine pack is imported from a file and then chosen', async ({ page }) => {
  // Doc 10 section 9 and doc 12 principle 4: a pack is data. Importing adds it
  // to the profile; switching to it is a separate act, so an import can never
  // re-judge the next card on its own.
  await importPack(page, packFile('house-rules', '0.2.0'));

  const row = page.locator('.lib-row[data-pack="house-rules"]');
  await expect(row).toBeVisible({ timeout: 30_000 });
  await expect(row).toContainText('Imported');
  await expect(row).not.toContainText('Active');

  await row.locator('[data-pack-act="use"]').click();
  await expect(row).toContainText('Active', { timeout: 30_000 });
  // And it is the core that says so: the page redraws from profile.get.
  await page.reload();
  await page.locator('#rail [data-view="profile"]').click();
  await expect(page.locator('.lib-row[data-pack="house-rules"]')).toContainText('Active');
});
