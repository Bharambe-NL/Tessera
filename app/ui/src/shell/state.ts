/**
 * The shell's state and the loop that moves it: reload, notifications, and the
 * one busy path every ask and rerun share.
 *
 * The state is one object rather than module level variables, because the
 * feature modules that set a board or a depth import this module, and an ES
 * import is read only: `state.boardId = id` works where `boardId = id` would
 * not compile.
 */

import { boundsOf, CARD_W, layout } from '../canvas/layout.js';
import { readingHTML } from '../canvas/reading.js';
import { drawEdges, measureHeights, renderCards, renderNotes } from '../canvas/render.js';
import type { Board, Depth } from '../canvas/types.js';
import { ViewportHost } from '../canvas/viewport.js';
import type { AnchorPopover } from '../popover.js';
import type { Router } from '../pages/router.js';
import { Rpc, RpcError, type AskAnchor, type Notification } from '../rpc.js';
import { COPY } from '../strings.js';
import {
  cardsEl,
  composer,
  edgesEl,
  emptyState,
  main,
  modeChip,
  modeLabel,
  packUpdate,
  readingEl,
  stickiesEl,
  titleInput,
  toasts,
  world,
} from './dom.js';

export const rpc = new Rpc();
export const viewport = new ViewportHost({ main, world, onSettled: () => void 0 });

/**
 * The two instances whose construction needs the feature modules: main.ts
 * builds them with callbacks from those modules and sets them here before boot.
 * Everything below runs after boot, so the late binding is never observed.
 */
export const linked = {} as { popover: AnchorPopover; router: Router };

export const state = {
  board: null as Board | null,
  boardId: null as string | null,
  depth: 'fast' as Depth,
  /** The alias the user picked from the composer's model control; empty is auto. */
  modelAlias: '',
  lastEventIndex: 0,
  /** Doc 11 section 10's list view alternative, when it is the one being read. */
  reading: false,
};

let heights = new Map<string, number>();
export const heightOf = (id: string) => heights.get(id) ?? 320;

export function renderReading(b: Board): void {
  readingEl.innerHTML = readingHTML(b);
}

/**
 * One full pass: lay out with the heights we know, render, measure what the
 * browser actually produced, then lay out again with the real heights and write
 * the corrected positions. Two passes because a card's height depends on its
 * content, and its neighbours' positions depend on its height.
 */
export function renderBoard(b: Board): void {
  if (state.reading) renderReading(b);
  layout(b.cards, heightOf);
  renderCards(b.cards, { cards: cardsEl, edges: edgesEl });

  heights = measureHeights(b.cards);
  layout(b.cards, heightOf);
  renderCards(b.cards, { cards: cardsEl, edges: edgesEl });
  renderNotes(b.notes ?? [], stickiesEl);
  drawEdges(b.cards, edgesEl, heightOf, b.notes ?? []);

  emptyState.hidden = b.cards.length > 0;
}

/**
 * Doc 16 section 3.6: keep the quote as a sticky beside the card it came from.
 *
 * The place is computed here rather than in the core, because where there is
 * room on the canvas is a question about the layout the reader is looking at
 * and the core has never seen it.
 */
export async function keepAsSticky(cardId: string, quote: string): Promise<void> {
  const id = state.boardId;
  if (!id) return;
  const card = state.board?.cards.find((c) => c.id === cardId);
  const position = card
    ? { x: Math.round(card.position.x + CARD_W + 80), y: Math.round(card.position.y + 24), w: 220, h: 140 }
    : { x: 560, y: 80, w: 220, h: 140 };
  try {
    await rpc.createNote(id, quote, { cardId, position });
    toast(COPY.stickyKept);
    await reload();
  } catch {
    toast(COPY.stickyFailed, 'error');
  }
}

export function setMode(label: string, tone: 'live' | 'busy' | 'offline'): void {
  modeLabel.textContent = label;
  modeChip.dataset.tone = tone;
}

/** Doc 11 section 9: errors say what happened and how to fix it. */
export function toast(message: string, level: 'info' | 'warn' | 'error' = 'info'): void {
  const node = document.createElement('div');
  node.className = `toast ${level}`;
  node.textContent = message;
  toasts.appendChild(node);
  window.setTimeout(() => node.remove(), 6000);
}

/**
 * Show a card that is running before its answer exists.
 *
 * The stage list is derived from events (doc 09 section 4), so it is the
 * bridge's notifications that fill it, never a guess made here.
 */
function placeholderCard(id: string, question: string, anchor: AskAnchor): void {
  if (!state.board) return;
  const parent = anchor.parentCardId ?? null;
  const anchored = Boolean(anchor.anchorText || anchor.anchorBlockRef);
  state.board.cards.push({
    id,
    parent_card_id: parent,
    // The same rule the core applies, so the placeholder draws the edge the
    // finished card will draw rather than jumping when the reload lands.
    kind: parent === null ? 'root' : anchored ? 'branch' : 'follow',
    anchor_text: anchor.anchorText ?? null,
    anchor_block_ref: anchor.anchorBlockRef ?? null,
    question,
    depth: state.depth,
    audience_id: null,
    answer: null,
    findings: [],
    visual: null,
    citations: [],
    flags: [],
    status: 'running',
    confidence: null,
    model_alias: null,
    stages: [],
    position: { x: 0, y: 0, dx: 0, dy: 0, pinned: false },
  });
  renderBoard(state.board);
}

/**
 * Set when a notification says the board changed in a way the read model has to
 * be re-read for.
 *
 * Pattern 25: the notification vocabulary is a view over the log, so most kinds
 * announce a change rather than carrying it. `flag_raised` names a rule and a
 * severity and not the reason the Flags queue shows, and `flag_resolved` names
 * only the card, so guessing the rest here would put a string on screen that no
 * event said. Re-reading the board is what turns the announcement into content.
 */
let staleRead = false;
/** Set when a notification changed how many flags are open. */
let flagCountStale = false;

function applyNotification(n: Notification): void {
  if (!state.board) return;
  if (n.kind === 'toast') {
    toast(n.message, n.level);
    return;
  }
  if (n.kind === 'board_updated') {
    staleRead = true;
    return;
  }

  const card = state.board.cards.find((c) => c.id === n.card_id);
  // A card the read model has never seen: the reload is what brings it in.
  if (!card) {
    staleRead = true;
    return;
  }

  switch (n.kind) {
    case 'card_stage': {
      const existing = card.stages.find((s) => s.label === n.label);
      if (existing) existing.done = n.done;
      else card.stages.push({ label: n.label, done: n.done });
      card.status = 'running';
      break;
    }
    // Terminal, and worth applying live rather than waiting: this is what stops
    // the card spinning the moment its answer lands.
    case 'card_answered': {
      card.status = n.status === 'flagged' ? 'flagged' : 'done';
      card.confidence = n.confidence;
      staleRead = true;
      break;
    }
    case 'card_failed': {
      card.status = 'failed';
      toast(n.reason, 'error');
      break;
    }
    case 'card_updated':
    case 'flag_raised':
    case 'flag_resolved': {
      staleRead = true;
      // The rail badge is the only part of the Flags queue visible from the
      // board, so it follows a flag the moment one is raised or cleared.
      flagCountStale = true;
      break;
    }
  }
}

/**
 * Read the bridge once and apply what it says.
 *
 * `allowReload` is false while an ask is in flight, because a reload replaces
 * the card array and would drop the placeholder standing in for the card being
 * written. The ask reloads once it finishes, so nothing is lost by waiting.
 */
export async function drainNotifications(allowReload = true): Promise<void> {
  if (!state.boardId) return;
  try {
    const { notifications, index } = await rpc.notifications(state.boardId, state.lastEventIndex);
    state.lastEventIndex = index;
    for (const n of notifications) applyNotification(n);
    if (flagCountStale) {
      flagCountStale = false;
      void linked.router.refreshFlagCount();
    }
    if (staleRead && allowReload) {
      staleRead = false;
      await reload();
      return;
    }
    if (state.board) renderBoard(state.board);
  } catch {
    // A dropped poll is not worth telling the user about; the reload after the
    // run finishes is the authority on what the card says.
  }
}

/**
 * Doc 10 section 9: the board offers the update, and nothing takes it on the
 * board's behalf.
 *
 * The version the board pinned is on the button, because "update pack" without
 * it asks a person to accept a change they cannot see the size of.
 */
export function showPackUpdate(board: Board): void {
  const update = board.pack_update;
  if (!update?.available) {
    packUpdate.hidden = true;
    return;
  }
  packUpdate.hidden = false;
  packUpdate.textContent =
    `${COPY.boardPackUpdate} (${update.pack_code} ${update.pinned_version} ` +
    `to ${update.current_version})`;
}

export async function reload(): Promise<void> {
  if (!state.boardId) return;
  staleRead = false;
  // Every rect the popover was placed against is about to be replaced.
  linked.popover.close();
  state.board = await rpc.getBoard(state.boardId);
  titleInput.value = state.board.title;
  showPackUpdate(state.board);
  renderBoard(state.board);
  viewport.fit(boundsOf(state.board.cards, heightOf));
}

/**
 * Run one call that writes a card, with the stage poll running underneath it.
 *
 * Ask and rerun differ only in the call and in whether a placeholder card is
 * standing in, so the busy state, the poll, the error and the reload are here
 * once rather than twice.
 */
export async function whileRunning<T>(work: () => Promise<T>, failure: string): Promise<T | null> {
  setMode(COPY.modeWorking, 'busy');
  composer.classList.add('busy');
  const poll = window.setInterval(() => void drainNotifications(false), 250);

  try {
    return await work();
  } catch (e) {
    toast(e instanceof RpcError ? e.message : failure, 'error');
    return null;
  } finally {
    window.clearInterval(poll);
    composer.classList.remove('busy');
    await reload();
    setMode(rpc.connected ? COPY.modeLive : COPY.modeOffline, rpc.connected ? 'live' : 'offline');
  }
}

export async function submit(question: string, anchor: AskAnchor = {}): Promise<void> {
  if (!state.boardId || !question.trim()) return;

  // The card exists before the answer does, so the reader sees it start. The
  // reload at the end of the run replaces it with the card the core wrote.
  placeholderCard(`pending-${Date.now()}`, question.trim(), anchor);

  const id = state.boardId;
  await whileRunning(
    () => rpc.ask(id, question.trim(), state.depth, anchor, state.modelAlias || undefined),
    COPY.askFailed,
  );
}
