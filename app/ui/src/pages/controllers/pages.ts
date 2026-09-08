/**
 * The Pages explorer's verbs. Doc 16 section 3.7.
 *
 * They all end the same way: the view redraws from what the core holds rather
 * than from what this call happened to return.
 */

import { RpcError } from '../../rpc.js';
import { COPY } from '../../strings.js';
import type { ControllerCtx } from './ctx.js';

export async function handle(action: string, el: HTMLElement, ctx: ControllerCtx): Promise<void> {
  switch (action) {
    case 'verb':
      await pageVerb(
        el.dataset.pageAct ?? '',
        el.closest<HTMLElement>('[data-page]')?.dataset.page,
        el.dataset.title,
        ctx,
      );
      return;
    case 'save':
      await savePage(ctx);
  }
}

async function pageVerb(
  verb: string,
  pageId: string | undefined,
  title: string | undefined,
  ctx: ControllerCtx,
): Promise<void> {
  try {
    switch (verb) {
      case 'new':
        // An unsaved page rather than a written one: a person who changes their
        // mind should not leave an empty page behind.
        ctx.state.pages = {
          open: {
            id: '',
            title: '',
            body: '',
            file_path: '',
            updated_at: '',
            source_card_id: null,
            citations_carried: [],
            links: [],
            backlinks: [],
          },
          editing: true,
        };
        break;
      case 'open':
        if (!pageId) return;
        ctx.state.pages = { open: await ctx.rpc.page(pageId), editing: false };
        break;
      case 'close':
        ctx.state.pages = { open: null, editing: false };
        break;
      case 'edit':
        ctx.state.pages = { ...ctx.state.pages, editing: true };
        break;
      case 'preview':
        // Read what the core holds rather than what the textarea shows: the
        // preview is of the page, and the page is what was saved.
        if (ctx.state.pages.open?.id) {
          ctx.state.pages = {
            open: await ctx.rpc.page(ctx.state.pages.open.id),
            editing: false,
          };
        } else {
          ctx.state.pages = { ...ctx.state.pages, editing: false };
        }
        break;
      case 'remove': {
        if (!pageId) return;
        await ctx.rpc.deletePage(pageId);
        ctx.state.pages = { open: null, editing: false };
        break;
      }
      case 'follow-link': {
        if (!title) return;
        const { pages } = await ctx.rpc.pages();
        const found = pages.find((p) => p.title.toLowerCase() === title.toLowerCase());
        if (found) ctx.state.pages = { open: await ctx.rpc.page(found.id), editing: false };
        break;
      }
      case 'create-link': {
        // Doc 16 section 3.1: an unresolved link creates the page it names, and
        // lands the person in it.
        if (!title) return;
        const made = await ctx.rpc.createPageFromLink(title);
        ctx.state.pages = { open: await ctx.rpc.page(made.page_id), editing: true };
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

async function savePage(ctx: ControllerCtx): Promise<void> {
  const title = ctx.body.querySelector<HTMLInputElement>('#page-name')?.value ?? '';
  const body = ctx.body.querySelector<HTMLTextAreaElement>('#page-text')?.value ?? '';
  const open = ctx.state.pages.open;
  try {
    const saved = await ctx.rpc.writePage({
      page_id: open?.id || undefined,
      title,
      body,
    });
    ctx.state.pages = { open: await ctx.rpc.page(saved.page_id), editing: false };
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.pagesWriteFailed, 'error');
  }
}
