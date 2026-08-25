/**
 * Entry point.
 *
 * At M0 this mounts the canvas layer against a fixture board and, with
 * `?gate=200`, runs the phase 0 acceptance gate from doc 12. From M3 the same
 * canvas layer renders cards projected from the core's event stream instead of
 * from a fixture, and this file gains the RPC client. The canvas modules do not
 * change when that happens: they already take a plain `Card[]`.
 */

import './styles/tokens.css';
import './styles/board.css';

import { boundsOf, layout } from './canvas/layout.js';
import { drawEdges, measureHeights, renderCards } from './canvas/render.js';
import type { Board } from './canvas/types.js';
import { ViewportHost } from './canvas/viewport.js';
import { makeBoard } from './perf/fixture.js';
import { formatResult, runGate } from './perf/gate.js';

const main = document.getElementById('main') as HTMLElement;
const world = document.getElementById('world') as HTMLElement;
const cardsEl = document.getElementById('cards') as HTMLElement;
const edgesEl = document.getElementById('edges') as unknown as SVGElement;
const gateEl = document.getElementById('gate') as HTMLPreElement;

const viewport = new ViewportHost({ main, world });
viewport.attach();

let heights = new Map<string, number>();
const heightOf = (id: string) => heights.get(id) ?? 320;

/**
 * One full pass: lay out with the heights we know, render, measure what the
 * browser actually produced, then lay out again with the real heights and write
 * the corrected positions. Two passes because a card's height depends on its
 * content, and its neighbours' positions depend on its height.
 */
function renderBoard(board: Board): void {
  layout(board.cards, heightOf);
  renderCards(board.cards, { cards: cardsEl, edges: edgesEl });

  heights = measureHeights(board.cards);
  layout(board.cards, heightOf);
  renderCards(board.cards, { cards: cardsEl, edges: edgesEl });
  drawEdges(board.cards, edgesEl, heightOf);
}

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

async function boot(): Promise<void> {
  const params = new URLSearchParams(location.search);
  const gateCards = Number(params.get('gate') ?? '0');
  const count = gateCards > 0 ? gateCards : Number(params.get('cards') ?? '24');

  const board = makeBoard(count);

  const t0 = performance.now();
  renderBoard(board);
  // Wait for the frame that actually paints what we just wrote.
  await new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
  const firstRenderMs = performance.now() - t0;

  viewport.fit(boundsOf(board.cards, heightOf));

  if (gateCards <= 0) return;

  const invoke = tauriInvoke();
  try {
    await whenVisible();
    const result = await runGate(
      board.cards.length,
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

void boot();
