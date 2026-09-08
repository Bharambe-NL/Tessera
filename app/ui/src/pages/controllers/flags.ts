/**
 * The Flags queue's verbs. Doc 09 sections 5, 6 and 14.
 *
 * Three ways in, and they share one decision. A row's own buttons decide that
 * row; the bulk bar decides everything selected; and the keyboard picks rows
 * without reaching for a checkbox. The selection lives in the page state, so it
 * survives the redraw that follows every decision.
 */

import { RpcError } from '../../rpc.js';
import { COPY } from '../../strings.js';
import { bulkHTML } from '../flags.js';
import type { ControllerCtx } from './ctx.js';

export async function handle(action: string, el: HTMLElement, ctx: ControllerCtx): Promise<void> {
  switch (action) {
    case 'bulk':
      await bulk(el.dataset.bulk ?? '', ctx);
      return;
    case 'select-board':
      selectGroup(el.dataset.selectBoard ?? '', ctx);
      await ctx.rerender();
      return;
    case 'row':
      await flagVerb(el, ctx);
  }
}

/**
 * Doc 09 section 14: flag rows navigable with arrows. Up and down move between
 * rows; Home and End go to the ends; Space picks the row for a bulk decision
 * without reaching for its checkbox.
 */
export function flagRowKeys(e: KeyboardEvent, body: HTMLElement): void {
  const row = (e.target as HTMLElement | null)?.closest<HTMLElement>('.flag-row');
  if (!row) return;

  const rows = [...body.querySelectorAll<HTMLElement>('.flag-row')];
  const at = rows.indexOf(row);
  if (at < 0) return;

  // Every key this handles assigns; every other key returns from the default
  // arm, so there is no path that reads an unset `next`.
  let next: number;
  switch (e.key) {
    case 'ArrowDown':
      next = Math.min(at + 1, rows.length - 1);
      break;
    case 'ArrowUp':
      next = Math.max(at - 1, 0);
      break;
    case 'Home':
      next = 0;
      break;
    case 'End':
      next = rows.length - 1;
      break;
    case ' ': {
      const box = row.querySelector<HTMLInputElement>('.pick');
      if (!box) return;
      e.preventDefault();
      box.checked = !box.checked;
      box.dispatchEvent(new Event('change', { bubbles: true }));
      return;
    }
    default:
      return;
  }
  // Off the end, focus stays where it is rather than wrapping into nothing.
  e.preventDefault();
  rows[next]?.focus();
}

/**
 * Row selection, which is a change rather than a click.
 *
 * The bulk bar redraws on its own here. A whole page render would re-read the
 * queue and take focus off the checkbox the person just used.
 */
export function pickChanged(box: HTMLInputElement, ctx: ControllerCtx): void {
  const id = box.closest<HTMLElement>('[data-flag]')?.dataset.flag;
  if (!id) return;
  if (box.checked) ctx.state.picked.add(id);
  else ctx.state.picked.delete(id);
  ctx.state.confirmingDismiss = false;
  drawBulk(ctx);
}

function drawBulk(ctx: ControllerCtx): void {
  ctx.tools.innerHTML = bulkHTML(ctx.state.picked.size, ctx.state.confirmingDismiss);
}

function selectGroup(boardId: string, ctx: ControllerCtx): void {
  for (const row of ctx.body.querySelectorAll<HTMLElement>(
    `.flag-group[data-board="${CSS.escape(boardId)}"] .flag-row`,
  )) {
    if (row.dataset.flag) ctx.state.picked.add(row.dataset.flag);
  }
}

async function flagVerb(button: HTMLElement, ctx: ControllerCtx): Promise<void> {
  const row = button.closest<HTMLElement>('[data-flag]');
  const id = row?.dataset.flag;
  if (!id) return;

  const verb = button.dataset.flagAct;
  if (verb === 'open') {
    // Doc 09 section 5's Open on a flag: go to the card.
    const boardId = row?.closest<HTMLElement>('.flag-group')?.dataset.board;
    if (boardId) await ctx.actions.openBoard(boardId);
    return;
  }
  if (verb === 'accept' || verb === 'dismiss' || verb === 'rerun') {
    await decide([id], verb, ctx);
  }
}

async function bulk(action: string, ctx: ControllerCtx): Promise<void> {
  const ids = [...ctx.state.picked];
  switch (action) {
    case 'accept':
      // Doc 09 section 6: bulk Accept needs no confirmation, because the flag
      // stands and the content stays hidden.
      await decide(ids, 'accept', ctx);
      return;
    case 'dismiss':
      // The first click asks; the second decides.
      ctx.state.confirmingDismiss = true;
      drawBulk(ctx);
      return;
    case 'dismiss-confirm':
      await decide(ids, 'dismiss', ctx);
      return;
    case 'clear':
      ctx.state.picked.clear();
      ctx.state.confirmingDismiss = false;
      await ctx.rerender();
  }
}

async function decide(
  ids: string[],
  decision: 'accept' | 'dismiss' | 'rerun',
  ctx: ControllerCtx,
): Promise<void> {
  if (ids.length === 0) return;
  try {
    await ctx.rpc.decideFlags(ids, decision);
    for (const id of ids) ctx.state.picked.delete(id);
    ctx.state.confirmingDismiss = false;
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.flagsFailed, 'error');
  }
}
