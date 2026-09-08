/**
 * The Library's verbs: the tab, and a decision on a proposed concept.
 *
 * The concept rows render here rather than on the Profile page, so the accept
 * and dismiss that sit on them belong to this controller.
 */

import { RpcError } from '../../rpc.js';
import { COPY } from '../../strings.js';
import type { ControllerCtx } from './ctx.js';

export async function handle(action: string, el: HTMLElement, ctx: ControllerCtx): Promise<void> {
  switch (action) {
    case 'tab': {
      const tab = el.dataset.libraryTab;
      if (tab !== 'sources' && tab !== 'concepts') return;
      ctx.state.libraryTab = tab;
      await ctx.rerender();
      return;
    }
    case 'decide': {
      const id = el.closest<HTMLElement>('[data-concept]')?.dataset.concept;
      const verb = el.dataset.conceptAct;
      if (id && (verb === 'accept' || verb === 'dismiss')) {
        await decideConcept(id, verb === 'accept', ctx);
      }
    }
  }
}

async function decideConcept(
  conceptId: string,
  accept: boolean,
  ctx: ControllerCtx,
): Promise<void> {
  try {
    await ctx.rpc.decideConcept(conceptId, accept);
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.conceptFailed, 'error');
  }
}
