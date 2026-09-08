/**
 * Home's verbs. Doc 11 section 5's board list.
 *
 * Every verb that writes ends by redrawing from a fresh read rather than from
 * what the call returned, because a board trashed here changes the count the
 * filter above it is showing.
 */

import { RpcError } from '../../rpc.js';
import { COPY } from '../../strings.js';
import type { ControllerCtx } from './ctx.js';

export async function handle(action: string, el: HTMLElement, ctx: ControllerCtx): Promise<void> {
  switch (action) {
    case 'filter': {
      const filter = el.dataset.homeFilter;
      if (filter !== 'active' && filter !== 'trashed') return;
      ctx.state.homeFilter = filter;
      await ctx.rerender();
      return;
    }
    case 'create':
      await ctx.actions.createBoard();
      return;
    case 'board':
      await boardVerb(el.dataset.boardAct ?? '', el.dataset.board ?? '', ctx);
  }
}

async function boardVerb(verb: string, boardId: string, ctx: ControllerCtx): Promise<void> {
  if (!boardId) return;
  try {
    switch (verb) {
      case 'open':
        await ctx.actions.openBoard(boardId);
        return;
      case 'trash':
        await ctx.rpc.trashBoard(boardId);
        break;
      case 'restore':
        await ctx.rpc.restoreBoard(boardId);
        break;
      case 'purge':
        await ctx.rpc.purgeBoard(boardId);
        break;
      default:
        return;
    }
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.boardVerbFailed, 'error');
  }
}
