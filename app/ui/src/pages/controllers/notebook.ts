/**
 * The Notebook's verbs. Doc 16 section 3.4.
 *
 * Every one ends by re-reading the session, because what a turn shows is what
 * the core recorded about it: the grounding state is on the event log and the
 * page chip is on the card.
 */

import { RpcError } from '../../rpc.js';
import { COPY } from '../../strings.js';
import type { ControllerCtx } from './ctx.js';

export async function handle(action: string, el: HTMLElement, ctx: ControllerCtx): Promise<void> {
  switch (action) {
    case 'turn':
      await notebookVerb(
        el.dataset.notebookAct ?? '',
        el.closest<HTMLElement>('[data-card]')?.dataset.card,
        ctx,
      );
      return;
    case 'ask':
      await askNotebook(ctx);
  }
}

async function notebookVerb(
  verb: string,
  cardId: string | undefined,
  ctx: ControllerCtx,
): Promise<void> {
  const boardId = ctx.state.notebook.session?.board_id;
  try {
    switch (verb) {
      case 'new': {
        const { board_id } = await ctx.rpc.openNotebook();
        ctx.state.notebook = {
          session: await ctx.rpc.notebookSession(board_id),
          asking: false,
        };
        break;
      }
      case 'save': {
        if (!boardId || !cardId) return;
        const saved = await ctx.rpc.saveAsPage(boardId, cardId);
        if (saved.created) ctx.actions.toast(`${COPY.savedAsPage}: ${saved.title ?? ''}`.trim());
        ctx.state.notebook = {
          session: await ctx.rpc.notebookSession(boardId),
          asking: false,
        };
        break;
      }
      case 'open-board': {
        if (!boardId || !cardId) return;
        const opened = await ctx.rpc.openOnBoard(boardId, cardId);
        await ctx.actions.openBoard(opened.board_id);
        return;
      }
      // Doc 16 section 3.4's one click way out of an ungrounded answer. The
      // first answer is superseded rather than replaced, so the session still
      // shows that the vault had nothing to say.
      case 'search-web': {
        if (!boardId || !cardId) return;
        ctx.actions.toast(COPY.notebookWebSearching);
        await ctx.rpc.searchWeb(boardId, cardId);
        ctx.state.notebook = {
          session: await ctx.rpc.notebookSession(boardId),
          asking: false,
        };
        break;
      }
      default:
        return;
    }
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
  }
}

async function askNotebook(ctx: ControllerCtx): Promise<void> {
  const input = ctx.body.querySelector<HTMLInputElement>('#notebook-question');
  const question = input?.value.trim() ?? '';
  if (!question) return;
  if (input) input.value = '';

  // A session on the first question, so a person who types before pressing the
  // button gets what they asked for rather than a refusal.
  let boardId = ctx.state.notebook.session?.board_id;
  try {
    if (!boardId) boardId = (await ctx.rpc.openNotebook()).board_id;
    ctx.state.notebook = { ...ctx.state.notebook, asking: true };
    await ctx.rerender();
    await ctx.rpc.ask(boardId, question, 'deep');
    ctx.state.notebook = {
      session: await ctx.rpc.notebookSession(boardId),
      asking: false,
    };
  } catch (e) {
    ctx.state.notebook = { ...ctx.state.notebook, asking: false };
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.askFailed, 'error');
  }
  await ctx.rerender();
}
