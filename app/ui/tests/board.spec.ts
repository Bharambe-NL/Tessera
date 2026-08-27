/**
 * Doc 12 phase 8: "every verb in 09 section 5 works and emits its event".
 *
 * These are the verbs M9 step 1 wires. A rendered screen and a working one are
 * different claims, and the gap between them is exactly what this file exists to
 * close: `render.ts` emitted `data-act="flags"`, `data-act="remove"` and
 * `data-act="follow"` for four milestones with no listener anywhere, so the
 * markup looked finished and nothing on the card responded.
 */

import { expect, test } from '@playwright/test';

import { cardCount, freshCore, useCore } from './shell.js';

const QUESTION = 'what are world models?';

test.beforeEach(async ({ page }) => {
  await freshCore(page);
  await useCore(page);
});

/** Ask from the composer and wait for the card the core wrote. */
async function askFirst(page: import('@playwright/test').Page): Promise<void> {
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');

  await page.locator('#ask').fill(QUESTION);
  await page.locator('#ask').press('Enter');

  await expect(page.locator('#cards .card')).toHaveCount(1, { timeout: 30_000 });
  await expect(page.locator('#cards .card .answer')).toBeVisible();
}

test('a question becomes a card the reader can see', async ({ page }) => {
  await askFirst(page);

  const card = page.locator('#cards .card').first();
  await expect(card.locator('.msg')).toHaveText(QUESTION);
  await expect(card.locator('.answer')).toContainText('world model');
  // The board takes its name from the first question. Doc 01 section 4.1.
  await expect(page.locator('#title')).toHaveValue(QUESTION);
});

test('the follow-up box on a card asks a follow-up', async ({ page }) => {
  await askFirst(page);
  const parent = page.locator('#cards .card').first();
  const parentId = await parent.getAttribute('data-card-id');

  await parent.locator('.followup').fill('which article says so?');
  await parent.locator('.followup').press('Enter');

  await expect(page.locator('#cards .card')).toHaveCount(2, { timeout: 30_000 });

  // The follow-up card answered. Counting cards is not enough: a card that
  // renders and fails is still a card, and the first version of this test
  // passed against a follow-up carrying "This card did not finish."
  const child = page.locator('#cards .card').nth(1);
  await expect(child).not.toHaveAttribute('data-status', 'failed');
  await expect(child.locator('.answer')).toContainText('world model');
  await expect(child.locator('.failed')).toHaveCount(0);

  // It names its parent rather than landing on the board as another root. This
  // is the assertion the RPC could not satisfy before M9: `card.ask` passed
  // `parent_card_id: None`.
  const cards = page.locator('#cards .card');
  const titles = await cards.locator('.head .title').allTextContents();
  expect(titles).toContain('Follow-up');

  const ids = await cards.evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.cardId));
  expect(ids.filter((id) => id === parentId)).toHaveLength(1);

  // An edge is drawn from parent to child, which is what makes it a board
  // rather than a list.
  const followEdge = await page.locator('#edges .edge.follow').getAttribute('d');
  expect(followEdge?.length ?? 0).toBeGreaterThan(0);
});

test('the flag chip opens the reason and closes it again', async ({ page }) => {
  await askFirst(page);
  const card = page.locator('#cards .card').first();

  // A fast card carries `fast_mode_notice`, so the chip is always there on this
  // board. Doc 07 section B8.1.
  const chip = card.locator('.chip.flag');
  await expect(chip).toBeVisible();
  await expect(chip).toHaveAttribute('aria-expanded', 'false');
  await expect(card.locator('.flag-list')).toHaveCount(0);

  await chip.click();
  await expect(card.locator('.flag-list li')).not.toHaveCount(0);
  await expect(card.locator('.flag-list .rule').first()).not.toBeEmpty();
  await expect(card.locator('.flag-list .reason').first()).not.toBeEmpty();

  await card.locator('.chip.flag').click();
  await expect(card.locator('.flag-list')).toHaveCount(0);
});

test('rerun checks the card again without writing a second one', async ({ page }) => {
  await askFirst(page);
  const before = await cardCount(page);
  const card = page.locator('#cards .card').first();
  const id = await card.getAttribute('data-card-id');

  await card.locator('[data-act="rerun"]').click();
  await expect(page.locator('#mode-label')).toHaveText('Live', { timeout: 30_000 });

  // Doc 09 section 5's Rerun on a card retrieves nothing and rewrites nothing.
  expect(await cardCount(page)).toBe(before);
  await expect(page.locator('#cards .card').first()).toHaveAttribute('data-card-id', id ?? '');
});

test('renaming the board stops the next question renaming it', async ({ page }) => {
  await askFirst(page);

  await page.locator('#title').fill('Capital rules');
  await page.locator('#title').press('Enter');

  await page.locator('#ask').fill('and what about the buffer?');
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card')).toHaveCount(2, { timeout: 30_000 });
  await expect(page.locator('#cards .card').nth(1)).not.toHaveAttribute('data-status', 'failed');

  // Doc 01 section 4.1: the inference stops once a person has typed a title.
  await expect(page.locator('#title')).toHaveValue('Capital rules');
});

test('selecting a span in an answer offers a branch, and asking makes one', async ({ page }) => {
  await askFirst(page);
  const parent = page.locator('#cards .card').first();
  const parentId = await parent.getAttribute('data-card-id');

  // Select "internal representation" inside the answer, the way a reader drags
  // across a phrase they want to pull on.
  await parent.locator('.answer').evaluate((el) => {
    const text = el.firstChild as Text;
    const at = text.data.indexOf('internal representation');
    const range = document.createRange();
    range.setStart(text, at);
    range.setEnd(text, at + 'internal representation'.length);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    el.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
  });

  const pop = page.locator('#anchor-pop');
  await expect(pop).toBeVisible();
  await expect(pop.locator('.anchor-label')).toHaveText('internal representation');
  await expect(pop.locator('#anchor-ask')).toHaveText('Ask about this');

  await pop.locator('#anchor-ask').click();
  await pop.locator('#anchor-question').fill('what does that representation contain?');
  await pop.locator('#anchor-branch').click();

  await expect(page.locator('#cards .card')).toHaveCount(2, { timeout: 30_000 });
  const child = page.locator('#cards .card').nth(1);
  await expect(child).not.toHaveAttribute('data-status', 'failed');

  // A branch takes its header from the anchor rather than from the question,
  // which is how a reader finds the span it came from. Doc 09 section 4.
  await expect(child.locator('.head .title')).toHaveText('internal representation');
  await expect(child.locator('.answer')).not.toBeEmpty();

  // Branch edges are drawn with the curve, not the orthogonal drop.
  const branchEdge = await page.locator('#edges .edge.branch').getAttribute('d');
  expect(branchEdge?.length ?? 0).toBeGreaterThan(0);

  const ids = await page.locator('#cards .card').evaluateAll((els) =>
    els.map((e) => (e as HTMLElement).dataset.cardId),
  );
  expect(ids[0]).toBe(parentId);
});

test('clicking a block of a visual offers to investigate it', async ({ page }) => {
  await askFirst(page);
  const parent = page.locator('#cards .card').first();

  // Every clickable block carries the JSON pointer the Visualizer wrote.
  const block = parent.locator('.vis .clk[data-ref]').first();
  await expect(block).toBeVisible();
  const ref = await block.getAttribute('data-ref');
  expect(ref?.startsWith('/')).toBe(true);

  await block.click();
  const pop = page.locator('#anchor-pop');
  await expect(pop).toBeVisible();
  // A block gets its own verb, because the anchor is a pointer and not a span.
  await expect(pop.locator('#anchor-ask')).toHaveText('Investigate this further');

  await pop.locator('#anchor-ask').click();
  await pop.locator('#anchor-question').fill('why is this part here?');
  await pop.locator('#anchor-branch').click();

  await expect(page.locator('#cards .card')).toHaveCount(2, { timeout: 30_000 });
  await expect(page.locator('#cards .card').nth(1)).not.toHaveAttribute('data-status', 'failed');
});

/** Ask something other than the default question and wait for its card. */
async function ask(page: import('@playwright/test').Page, question: string): Promise<void> {
  await page.goto('/');
  await expect(page.locator('#mode-label')).toHaveText('Live');
  await page.locator('#ask').fill(question);
  await page.locator('#ask').press('Enter');
  await expect(page.locator('#cards .card')).toHaveCount(1, { timeout: 30_000 });
  await expect(page.locator('#cards .card .answer')).toBeVisible();
}

test('a summary that loops is drawn as a flow, with the edge that goes back', async ({ page }) => {
  // Doc 16 section 3.5: a tree has no cycles, so the edge returning to the
  // draft is the one a tree could not have shown at all.
  await ask(page, 'how does the review loop work?');
  const card = page.locator('#cards .card').first();

  const flow = card.locator('.vis.flow');
  await expect(flow).toBeVisible();
  await expect(flow.locator('.node[data-ref="/nodes/0"]')).toHaveText('Draft');
  await expect(flow.locator('.edges .edge')).toHaveCount(2);
  await expect(flow.locator('.edge .how[data-ref="/edges/1"]')).toHaveText('returns to');

  // And a node is a block like any other, so it can be investigated.
  await flow.locator('.node[data-ref="/nodes/1"]').click();
  await expect(page.locator('#anchor-pop')).toBeVisible();
});

test('two quantities are drawn as tiles', async ({ page }) => {
  await ask(page, 'tell me about the hall in numbers');
  const card = page.locator('#cards .card').first();

  const tiles = card.locator('.vis .tiles .tile');
  await expect(tiles).toHaveCount(2);
  await expect(tiles.nth(0)).toContainText('1949');
  // The unit sits with its numeral rather than in the label under it.
  await expect(tiles.nth(1).locator('b i')).toHaveText('m');
  await expect(tiles.nth(1).locator('span')).toHaveText('floor space');
  await expect(tiles.nth(0)).toHaveAttribute('data-ref', '/tiles/0');
});

test('a quote is kept as a sticky, attached to its card, and taken off again', async ({ page }) => {
  // Doc 16 section 3.6: "Add note" from the highlight menu, with the quote
  // prefilled, attached by a dashed edge.
  await askFirst(page);
  const card = page.locator('#cards .card').first();

  await card.locator('.answer').evaluate((el) => {
    const text = el.firstChild as Text;
    const at = text.data.indexOf('internal representation');
    const range = document.createRange();
    range.setStart(text, at);
    range.setEnd(text, at + 'internal representation'.length);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    el.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
  });

  const pop = page.locator('#anchor-pop');
  await expect(pop).toBeVisible();
  await pop.locator('#anchor-note').click();

  const sticky = page.locator('#stickies .sticky');
  await expect(sticky).toHaveCount(1);
  await expect(sticky).toContainText('internal representation');
  // The dashed edge is drawn from the card it quotes.
  await expect(page.locator('#edges .edge.quoted')).toHaveAttribute('d', /^M/);

  // Doc 09 section 5: every verb has an undo, and this is Add note's.
  await sticky.locator('[data-act="unstick"]').click();
  await expect(page.locator('#stickies .sticky')).toHaveCount(0);
});

test('hovering a card offers four handles, and one puts the cursor in the follow-up', async ({
  page,
}) => {
  // Doc 16 section 3.6: four handles on hover, which do what the card's footer
  // input does. A Card carries a question and the store requires one, so there
  // is no empty card for a handle to make.
  await askFirst(page);
  const card = page.locator('#cards .card').first();
  const handles = page.locator('#handles');

  await expect(handles).toBeHidden();
  await card.hover();
  await expect(handles).toBeVisible();
  await expect(handles.locator('button')).toHaveCount(4);

  // The handles are not in the card's own markup, which is what doc 12 phase
  // 0's pan gate depends on: a hover that rebuilt a card would put the render
  // diff back where it was.
  const markup = await card.innerHTML();
  expect(markup).not.toContain('data-side');

  await handles.locator('[data-side="right"]').click();
  await expect(card.locator('.followup')).toBeFocused();

  // And the follow-up it focuses is the one that works.
  await card.locator('.followup').fill('which article says so?');
  await card.locator('.followup').press('Enter');
  await expect(page.locator('#cards .card')).toHaveCount(2, { timeout: 30_000 });
});

test('escape puts the popover away without asking anything', async ({ page }) => {
  await askFirst(page);
  const parent = page.locator('#cards .card').first();

  await parent.locator('.vis .clk[data-ref]').first().click();
  await expect(page.locator('#anchor-pop')).toBeVisible();

  await page.locator('#anchor-pop #anchor-ask').click();
  await page.locator('#anchor-question').press('Escape');

  await expect(page.locator('#anchor-pop')).toBeHidden();
  expect(await cardCount(page)).toBe(1);
});

test('how this was built reads the event log', async ({ page }) => {
  await askFirst(page);
  const card = page.locator('#cards .card').first();
  const built = card.locator('details.built');

  // `board.history` was registered on the core at M2 and called by nothing, so
  // this disclosure opened onto an empty div for four milestones.
  await expect(card.locator('.built-body')).toBeEmpty();
  await built.locator('summary').click();

  const rows = card.locator('.built-row');
  await expect(rows.first()).toBeVisible();

  // The model calls are named by their stage, with the model and the tokens
  // each one cost, because tokens are what the log records.
  const terms = await card.locator('.built-row dt').allTextContents();
  expect(terms).toContain('Routed');
  expect(terms).toContain('synthesize');
  expect(terms).toContain('visualize');
  await expect(card.locator('.built-total')).toContainText('tokens');

  // The Verified row counts what the event recorded. On a fast card, which
  // cites nothing, it must not claim the answer was checked against sources:
  // the first version of this row said exactly that on every card.
  const verified = card.locator('.built-row', { has: page.locator('dt:text-is("Verified")') });
  await expect(verified.locator('dd')).toContainText('rules passed');
  await expect(verified.locator('dd')).not.toContainText('citations supported');
});

test('no card on the board reports a failure', async ({ page }) => {
  // The guard the other tests needed and did not have. A card that renders is
  // not a card that answered, and every assertion above counts cards or reads
  // one field; this one reads every card's terminal state and every error the
  // shell surfaced, so a fixture that runs dry fails here first.
  await askFirst(page);
  const parent = page.locator('#cards .card').first();

  for (const question of ['which article says so?', 'and the buffer?']) {
    await parent.locator('.followup').fill(question);
    await parent.locator('.followup').press('Enter');
    await expect(page.locator('#mode-label')).toHaveText('Live', { timeout: 30_000 });
  }

  expect(await cardCount(page)).toBe(3);
  await expect(page.locator('#cards .card[data-status="failed"]')).toHaveCount(0);
  await expect(page.locator('#cards .failed')).toHaveCount(0);
  await expect(page.locator('#toasts .toast.error')).toHaveCount(0);
});

test('a card is kept as a page and says so afterwards', async ({ page }) => {
  // Doc 16 section 3.2's ninth verb. The chip is what tells a person the card
  // is now somewhere they own, and the verb goes away because there is nothing
  // left to do to it.
  await askFirst(page);

  const card = page.locator('#cards .card').first();
  await expect(card.locator('.save')).toBeVisible();
  await expect(card.locator('.chip.page')).toHaveCount(0);

  await card.locator('.save').click();
  await expect(card.locator('.chip.page')).toBeVisible({ timeout: 30_000 });
  await expect(card.locator('.chip.page')).toHaveText('In my pages');
  await expect(card.locator('.save')).toHaveCount(0, {
    timeout: 30_000,
  });

  // And it is the core that says so: the chip survives a reload.
  await page.reload();
  await expect(page.locator('#cards .card .chip.page')).toBeVisible({ timeout: 30_000 });
});

test('a card the reader dwells on is reported as read, once', async ({ page }) => {
  // Doc 17 section 2.2: "a card that links the concept is read" is what moves
  // a concept from unseen to exposed, and only the shell can see reading. Doc
  // 17 open question 2 settles what reading means at a three second dwell.
  const viewed: string[] = [];
  page.on('request', (request) => {
    if (!request.url().endsWith('/rpc')) return;
    const body = request.postData() ?? '';
    if (body.includes('"card.viewed"')) viewed.push(body);
  });

  await askFirst(page);
  // Nothing yet: a card that has just appeared has not been read.
  expect(viewed).toHaveLength(0);

  // Past the dwell, and then well past it, because the report happens once per
  // card and a second one would be the log filling up with scrolling.
  await expect(async () => {
    expect(viewed).toHaveLength(1);
  }).toPass({ timeout: 15_000 });
  await page.waitForTimeout(4_000);
  expect(viewed).toHaveLength(1);
});
