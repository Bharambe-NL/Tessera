/**
 * Entry point.
 *
 * The canvas layer renders whatever `Card[]` it is handed, so this file is the
 * only place that knows a core exists. With `?gate=200` it runs the doc 12
 * phase 0 acceptance gate against a fixture board instead, which is why the
 * fixture and the RPC path both land here and nowhere else.
 */

import './styles/tokens.css';
import './styles/board.css';
import './styles/chrome.css';

import { boundsOf, layout } from './canvas/layout.js';
import { drawEdges, measureHeights, renderCards, toggleFlags } from './canvas/render.js';
import type { Board, Depth } from './canvas/types.js';
import { ViewportHost } from './canvas/viewport.js';
import { makeBoard } from './perf/fixture.js';
import { formatResult, runGate } from './perf/gate.js';
import { Rpc, RpcError, type AskAnchor, type Notification } from './rpc.js';
import { COPY, PRODUCT_NAME } from './strings.js';

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const main = el<HTMLElement>('main');
const world = el<HTMLElement>('world');
const cardsEl = el<HTMLElement>('cards');
const edgesEl = document.getElementById('edges') as unknown as SVGElement;
const gateEl = el<HTMLPreElement>('gate');
const composer = el<HTMLFormElement>('composer');
const ask = el<HTMLTextAreaElement>('ask');
const titleInput = el<HTMLInputElement>('title');
const modeLabel = el<HTMLElement>('mode-label');
const modeChip = el<HTMLElement>('mode');
const emptyState = el<HTMLElement>('empty');
const toasts = el<HTMLElement>('toasts');

const rpc = new Rpc();
const viewport = new ViewportHost({ main, world, onSettled: () => void 0 });
viewport.attach();

let heights = new Map<string, number>();
const heightOf = (id: string) => heights.get(id) ?? 320;

let board: Board | null = null;
let boardId: string | null = null;
let depth: Depth = 'fast';
let lastEventIndex = 0;

/**
 * One full pass: lay out with the heights we know, render, measure what the
 * browser actually produced, then lay out again with the real heights and write
 * the corrected positions. Two passes because a card's height depends on its
 * content, and its neighbours' positions depend on its height.
 */
function renderBoard(b: Board): void {
  layout(b.cards, heightOf);
  renderCards(b.cards, { cards: cardsEl, edges: edgesEl });

  heights = measureHeights(b.cards);
  layout(b.cards, heightOf);
  renderCards(b.cards, { cards: cardsEl, edges: edgesEl });
  drawEdges(b.cards, edgesEl, heightOf);

  emptyState.hidden = b.cards.length > 0;
}

function setMode(label: string, tone: 'live' | 'busy' | 'offline'): void {
  modeLabel.textContent = label;
  modeChip.dataset.tone = tone;
}

/** Doc 11 section 9: errors say what happened and how to fix it. */
function toast(message: string, level: 'info' | 'warn' | 'error' = 'info'): void {
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
  if (!board) return;
  const parent = anchor.parentCardId ?? null;
  const anchored = Boolean(anchor.anchorText || anchor.anchorBlockRef);
  board.cards.push({
    id,
    parent_card_id: parent,
    // The same rule the core applies, so the placeholder draws the edge the
    // finished card will draw rather than jumping when the reload lands.
    kind: parent === null ? 'root' : anchored ? 'branch' : 'follow',
    anchor_text: anchor.anchorText ?? null,
    anchor_block_ref: anchor.anchorBlockRef ?? null,
    question,
    depth,
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
  renderBoard(board);
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

function applyNotification(n: Notification): void {
  if (!board) return;
  if (n.kind === 'toast') {
    toast(n.message, n.level);
    return;
  }
  if (n.kind === 'board_updated') {
    staleRead = true;
    return;
  }

  const card = board.cards.find((c) => c.id === n.card_id);
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
async function drainNotifications(allowReload = true): Promise<void> {
  if (!boardId) return;
  try {
    const { notifications, index } = await rpc.notifications(boardId, lastEventIndex);
    lastEventIndex = index;
    for (const n of notifications) applyNotification(n);
    if (staleRead && allowReload) {
      staleRead = false;
      await reload();
      return;
    }
    if (board) renderBoard(board);
  } catch {
    // A dropped poll is not worth telling the user about; the reload after the
    // run finishes is the authority on what the card says.
  }
}

async function reload(): Promise<void> {
  if (!boardId) return;
  staleRead = false;
  board = await rpc.getBoard(boardId);
  titleInput.value = board.title;
  renderBoard(board);
  viewport.fit(boundsOf(board.cards, heightOf));
}

/**
 * Run one call that writes a card, with the stage poll running underneath it.
 *
 * Ask and rerun differ only in the call and in whether a placeholder card is
 * standing in, so the busy state, the poll, the error and the reload are here
 * once rather than twice.
 */
async function whileRunning<T>(work: () => Promise<T>, failure: string): Promise<T | null> {
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

async function submit(question: string, anchor: AskAnchor = {}): Promise<void> {
  if (!boardId || !question.trim()) return;

  // The card exists before the answer does, so the reader sees it start. The
  // reload at the end of the run replaces it with the card the core wrote.
  placeholderCard(`pending-${Date.now()}`, question.trim(), anchor);

  const id = boardId;
  await whileRunning(() => rpc.ask(id, question.trim(), depth, anchor), COPY.askFailed);
}

/** Clear the composer and ask. Its own input, so its own reset. */
function submitFromComposer(): void {
  const question = ask.value;
  ask.value = '';
  ask.style.height = 'auto';
  void submit(question);
}

/**
 * One listener for every verb on every card. Doc 09 section 5.
 *
 * Delegation rather than a handler per card because `renderCards` rebuilds a
 * card's markup whenever its signature changes, and a listener bound to an
 * element that gets replaced stops firing without saying so. This one is bound
 * to the container, which is never replaced.
 */
function wireCardActions(): void {
  cardsEl.addEventListener('click', (e) => {
    const target = e.target as HTMLElement | null;
    const button = target?.closest<HTMLElement>('[data-act]');
    const cardEl = target?.closest<HTMLElement>('.card');
    const cardId = cardEl?.dataset.cardId;
    if (!button || !cardId || !boardId) return;

    switch (button.dataset.act) {
      case 'flags': {
        toggleFlags(cardId);
        if (board) renderBoard(board);
        break;
      }
      case 'follow': {
        const input = cardEl?.querySelector<HTMLInputElement>('.followup');
        const question = input?.value ?? '';
        if (!question.trim()) {
          input?.focus();
          break;
        }
        if (input) input.value = '';
        void submit(question, { parentCardId: cardId });
        break;
      }
      case 'rerun': {
        const id = boardId;
        void whileRunning(() => rpc.verify(id, cardId), COPY.rerunFailed);
        break;
      }
    }
  });

  // Enter in a card's follow-up box sends it, matching the composer.
  cardsEl.addEventListener('keydown', (e) => {
    const input = (e.target as HTMLElement | null)?.closest<HTMLInputElement>('.followup');
    if (!input || e.key !== 'Enter') return;
    e.preventDefault();
    const cardId = input.closest<HTMLElement>('.card')?.dataset.cardId;
    if (!cardId || !input.value.trim()) return;
    const question = input.value;
    input.value = '';
    void submit(question, { parentCardId: cardId });
  });
}

/**
 * Rename the board on blur or Enter.
 *
 * Doc 01 section 4.1: a board takes its title from the first question until a
 * person names it, and this is what stops that inference.
 */
function wireTitle(): void {
  const commit = () => {
    const title = titleInput.value.trim();
    if (!boardId || !board || !title || title === board.title) return;
    const id = boardId;
    void rpc
      .rename(id, title)
      .then(() => {
        if (board) board.title = title;
      })
      .catch((e: unknown) => {
        toast(e instanceof RpcError ? e.message : COPY.renameFailed, 'error');
        if (board) titleInput.value = board.title;
      });
  };

  titleInput.addEventListener('blur', commit);
  titleInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      titleInput.blur();
    }
  });
}

function wireComposer(): void {
  composer.addEventListener('submit', (e) => {
    e.preventDefault();
    submitFromComposer();
  });

  ask.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submitFromComposer();
    }
  });

  // Grow with the question rather than scrolling a one line box.
  ask.addEventListener('input', () => {
    ask.style.height = 'auto';
    ask.style.height = `${Math.min(ask.scrollHeight, 160)}px`;
  });

  for (const button of document.querySelectorAll<HTMLButtonElement>('#modes button')) {
    button.addEventListener('click', () => {
      depth = (button.dataset.depth as Depth) ?? 'fast';
      for (const b of document.querySelectorAll('#modes button')) b.classList.remove('on');
      button.classList.add('on');
    });
  }

  el<HTMLButtonElement>('zoom-in').addEventListener('click', () => viewport.zoomCentre(1.25));
  el<HTMLButtonElement>('zoom-out').addEventListener('click', () => viewport.zoomCentre(0.8));
  el<HTMLButtonElement>('fit').addEventListener('click', () => {
    if (board) viewport.fit(boundsOf(board.cards, heightOf));
  });
  el<HTMLButtonElement>('tidy').addEventListener('click', () => {
    if (!board) return;
    for (const c of board.cards) {
      c.position.dx = 0;
      c.position.dy = 0;
      c.position.pinned = false;
    }
    renderBoard(board);
  });
}

// ------------------------------------------------------------------- gate --

/** The Tauri command bridge, present only inside the shell. */
type Invoke = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;
function tauriInvoke(): Invoke | null {
  const g = window as unknown as { __TAURI__?: { core?: { invoke?: Invoke } } };
  return g.__TAURI__?.core?.invoke ?? null;
}

/** Resolve once the document is actually visible, so frames are scheduled. */
function whenVisible(timeoutMs = 10_000): Promise<void> {
  if (document.visibilityState === 'visible') return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => {
      document.removeEventListener('visibilitychange', onChange);
      reject(new Error('The window never became visible, so no frames were scheduled.'));
    }, timeoutMs);
    const onChange = () => {
      if (document.visibilityState !== 'visible') return;
      window.clearTimeout(timer);
      document.removeEventListener('visibilitychange', onChange);
      resolve();
    };
    document.addEventListener('visibilitychange', onChange);
  });
}

async function runPerfGate(cardCount: number): Promise<void> {
  const fixture = makeBoard(cardCount);
  composer.hidden = true;

  const t0 = performance.now();
  renderBoard(fixture);
  await new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
  const firstRenderMs = performance.now() - t0;

  viewport.fit(boundsOf(fixture.cards, heightOf));

  const invoke = tauriInvoke();
  try {
    await whenVisible();
    const result = await runGate(
      fixture.cards.length,
      {
        panBy: (dx, dy) => viewport.panBy(dx, dy),
        zoomCentre: (f) => viewport.zoomCentre(f),
        flush: () => viewport.applySync(),
      },
      firstRenderMs,
    );
    const text = formatResult(result);
    gateEl.hidden = false;
    gateEl.textContent = text;
    (window as unknown as { __gate: unknown }).__gate = result;
    if (invoke) await invoke('report_gate', { text, passed: result.passed, raw: result });
    else console.log(text);
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    gateEl.hidden = false;
    gateEl.textContent = `gate could not run: ${message}`;
    if (invoke) await invoke('report_gate_error', { message });
    else console.error(message);
  }
}

// ------------------------------------------------------------------- boot --

async function boot(): Promise<void> {
  // BN-024: the name has one home in code. index.html carries a static title so
  // the tab is not blank before this runs, and this is what it settles on.
  document.title = PRODUCT_NAME;

  const params = new URLSearchParams(location.search);

  const gateCards = Number(params.get('gate') ?? '0');
  if (gateCards > 0) {
    await runPerfGate(gateCards);
    return;
  }

  wireComposer();
  wireCardActions();
  wireTitle();

  // No core behind the page: a plain browser can still see the canvas render,
  // which is what the fixture is for, but it cannot ask anything.
  if (!rpc.connected) {
    setMode(COPY.modeOffline, 'offline');
    const fixture = makeBoard(Number(params.get('cards') ?? '6'));
    board = fixture;
    titleInput.value = fixture.title;
    renderBoard(fixture);
    viewport.fit(boundsOf(fixture.cards, heightOf));
    composer.classList.add('disabled');
    ask.placeholder = COPY.askOffline;
    ask.disabled = true;
    return;
  }

  try {
    const { boards } = await rpc.listBoards();
    boardId = boards[0]?.id ?? (await rpc.createBoard()).board_id;
    await reload();
    setMode(COPY.modeLive, 'live');
    ask.focus();
  } catch (e) {
    const message = e instanceof RpcError ? e.message : COPY.coreSilent;
    setMode(COPY.modeOffline, 'offline');
    toast(message, 'error');
  }
}

void boot();
