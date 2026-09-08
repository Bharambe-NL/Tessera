/**
 * The Map's verbs. Doc 17 sections 2, 3 and 6.
 *
 * Three kinds of thing happen here: the map itself is filtered and its nodes
 * opened, the placement pass rates a tile or passes over it, and an open node
 * hands the learner to the board with a lesson, a check or a question.
 */

import { RpcError } from '../../rpc.js';
import { COPY } from '../../strings.js';
import type { MapFilter } from '../map.js';
import type { ControllerCtx } from './ctx.js';

export async function handle(action: string, el: HTMLElement, ctx: ControllerCtx): Promise<void> {
  switch (action) {
    case 'filter': {
      const filter = el.dataset.mapFilter;
      if (!filter) return;
      ctx.state.map.filter = filter as MapFilter;
      await ctx.rerender();
      return;
    }
    case 'open-node':
      await openNode(el.dataset.concept ?? '', ctx);
      return;
    case 'verb': {
      const verb = el.dataset.mapAct;
      if (!verb) return;
      await mapVerb(verb, ctx);
      return;
    }
    case 'place-skip': {
      const conceptId = el.dataset.placeSkip;
      if (!conceptId) return;
      await place(conceptId, null, ctx);
      return;
    }
    case 'place-rate': {
      const conceptId = el.dataset.placeConcept;
      if (!conceptId) return;
      await place(conceptId, Number(el.dataset.placeRate), ctx);
      return;
    }
    case 'rate':
      await rate(Number(el.dataset.mapRate), ctx);
      return;
    case 'open-board': {
      const boardId = el.dataset.mapCard;
      if (!boardId) return;
      await ctx.actions.openBoard(boardId);
      return;
    }
    case 'open-page': {
      const pageId = el.dataset.mapPage;
      if (!pageId) return;
      await openPage(pageId, ctx);
    }
  }
}

/** Doc 17 section 6's node panel, with its links read as it opens. */
async function openNode(conceptId: string, ctx: ControllerCtx): Promise<void> {
  const concept = ctx.state.map.map?.concepts.find((c) => c.concept_id === conceptId);
  if (!concept) return;
  ctx.state.map.open = concept;
  ctx.state.map.links = null;
  await ctx.rerender();
  try {
    ctx.state.map.links = await ctx.rpc.mapConcept(conceptId);
    await ctx.rerender();
  } catch {
    // The panel stands without its links rather than closing: the rating and
    // the three verbs are what a learner came for, and they need no read.
  }
}

async function mapVerb(verb: string, ctx: ControllerCtx): Promise<void> {
  if (verb === 'close') {
    ctx.state.map.open = null;
    ctx.state.map.links = null;
    await ctx.rerender();
    return;
  }
  if (verb === 'mission') {
    ctx.state.map.missionOnly = !ctx.state.map.missionOnly;
    await ctx.rerender();
    return;
  }
  if (verb === 'place' || verb === 'placed') {
    ctx.state.map.placing = verb === 'place';
    // Going back into placement brings the skipped tiles with it: a skip is
    // "not now" rather than "never ask me", and the learner asked again.
    if (verb === 'place') ctx.state.map.skipped = [];
    await ctx.rerender();
    return;
  }

  const concept = ctx.state.map.open;
  if (!concept) return;
  try {
    // Doc 17 section 6's three verbs, each of which lands the learner on a
    // board. A lesson and a check both start a session; the check asks its
    // first question straight away, which is what "check me now" means.
    if (verb === 'explore') {
      const { board_id } = await ctx.rpc.createBoard(concept.term);
      await ctx.actions.openBoard(board_id);
      ctx.actions.ask(concept.term);
      return;
    }
    if (verb === 'lesson' || verb === 'check') {
      await ctx.actions.startLesson(concept.term, verb === 'check');
    }
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.mapFailed, 'error');
  }
}

/** Doc 17 section 3: one tile of the placement pass, rated or passed over. */
async function place(conceptId: string, rating: number | null, ctx: ControllerCtx): Promise<void> {
  if (rating === null) {
    ctx.state.map.skipped = [...ctx.state.map.skipped, conceptId];
    await ctx.rerender();
    return;
  }
  try {
    await ctx.rpc.rateConcept(conceptId, rating);
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.mapFailed, 'error');
  }
}

/** Doc 17 section 2.1: the learner rates, and it is a claim, never evidence. */
async function rate(rating: number, ctx: ControllerCtx): Promise<void> {
  const concept = ctx.state.map.open;
  if (!concept || !Number.isFinite(rating)) return;
  try {
    await ctx.rpc.rateConcept(concept.concept_id, rating);
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.mapFailed, 'error');
  }
}

async function openPage(pageId: string, ctx: ControllerCtx): Promise<void> {
  try {
    ctx.state.pages = { open: await ctx.rpc.page(pageId), editing: false };
    await ctx.go('pages');
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
  }
}
