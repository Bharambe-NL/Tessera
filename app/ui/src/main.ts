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
import { drawEdges, measureHeights, renderCards } from './canvas/render.js';
import type { Board, Depth } from './canvas/types.js';
import { ViewportHost } from './canvas/viewport.js';
import { makeBoard } from './perf/fixture.js';
import { formatResult, runGate } from './perf/gate.js';
import { Rpc, RpcError, type Notification } from './rpc.js';

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
function placeholderCard(id: string, question: string): void {
  if (!board) return;
  board.cards.push({
    id,
    parent_card_id: null,
    kind: 'root',
    anchor_text: null,
    anchor_block_ref: null,
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

function applyNotification(n: Notification): void {
  if (!board) return;
  if (n.kind === 'toast') {
    toast(n.message, n.level);
    return;
  }
  if (!('card_id' in n)) return;

  const card = board.cards.find((c) => c.id === n.card_id);
  if (!card) return;

  if (n.kind === 'card_stage') {
    const existing = card.stages.find((s) => s.label === n.label);
    if (existing) existing.done = n.done;
    else card.stages.push({ label: n.label, done: n.done });
    card.status = 'running';
  }
}

async function drainNotifications(): Promise<void> {
  if (!boardId) return;
  try {
    const { notifications, index } = await rpc.notifications(boardId, lastEventIndex);
    lastEventIndex = index;
    for (const n of notifications) applyNotification(n);
    if (board) renderBoard(board);
  } catch {
    // A dropped poll is not worth telling the user about; the reload after the
    // run finishes is the authority on what the card says.
  }
}

async function reload(): Promise<void> {
  if (!boardId) return;
  board = await rpc.getBoard(boardId);
  titleInput.value = board.title;
  renderBoard(board);
  viewport.fit(boundsOf(board.cards, heightOf));
}

async function submit(question: string): Promise<void> {
  if (!boardId || !question.trim()) return;

  ask.value = '';
  ask.style.height = 'auto';
  setMode('Working', 'busy');
  composer.classList.add('busy');

  // The card exists before the answer does, so the reader sees it start.
  const provisional = `pending-${Date.now()}`;
  placeholderCard(provisional, question.trim());

  // Poll the bridge while the run is in flight. The core answers the ask
  // synchronously, so this is what fills the stage list in the meantime.
  const poll = window.setInterval(() => void drainNotifications(), 250);

  try {
    await rpc.ask(boardId, question.trim(), depth);
  } catch (e) {
    const message = e instanceof RpcError ? e.message : 'That card did not finish.';
    toast(message, 'error');
  } finally {
    window.clearInterval(poll);
    composer.classList.remove('busy');
    if (board) board.cards = board.cards.filter((c) => c.id !== provisional);
    await reload();
    setMode(rpc.connected ? 'Live' : 'Offline', rpc.connected ? 'live' : 'offline');
  }
}

function wireComposer(): void {
  composer.addEventListener('submit', (e) => {
    e.preventDefault();
    void submit(ask.value);
  });

  ask.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void submit(ask.value);
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
  const params = new URLSearchParams(location.search);

  const gateCards = Number(params.get('gate') ?? '0');
  if (gateCards > 0) {
    await runPerfGate(gateCards);
    return;
  }

  wireComposer();

  // No core behind the page: a plain browser can still see the canvas render,
  // which is what the fixture is for, but it cannot ask anything.
  if (!rpc.connected) {
    setMode('Offline', 'offline');
    const fixture = makeBoard(Number(params.get('cards') ?? '6'));
    board = fixture;
    titleInput.value = fixture.title;
    renderBoard(fixture);
    viewport.fit(boundsOf(fixture.cards, heightOf));
    composer.classList.add('disabled');
    ask.placeholder = 'Open Tessera to ask a question';
    ask.disabled = true;
    return;
  }

  try {
    const { boards } = await rpc.listBoards();
    boardId = boards[0]?.id ?? (await rpc.createBoard()).board_id;
    await reload();
    setMode('Live', 'live');
    ask.focus();
  } catch (e) {
    const message = e instanceof RpcError ? e.message : 'The core did not answer.';
    setMode('Offline', 'offline');
    toast(message, 'error');
  }
}

void boot();
