/**
 * Entry point, and nothing but composition.
 *
 * The shell's state and verbs live in `shell/`; this file builds the two
 * instances whose callbacks cross feature lines (the popover and the router),
 * wires every feature at boot, and runs the doc 12 phase 0 acceptance gate when
 * asked with `?gate=200`, which is why the fixture and the gate land here and
 * nowhere else.
 */

import './styles/fonts.css';
import './styles/tokens.css';
import './styles/components.css';
import './styles/board.css';
import './styles/chrome.css';
import './styles/pages.css';

import { watchExposure } from './exposure.js';
import { boundsOf } from './canvas/layout.js';
import { makeBoard } from './perf/fixture.js';
import { formatResult, runGate } from './perf/gate.js';
import { Router } from './pages/router.js';
import { AnchorPopover } from './popover.js';
import { RpcError } from './rpc.js';
import { COPY, PRODUCT_NAME } from './strings.js';
import {
  wireBranching,
  wireBuildTrail,
  wireCardActions,
  wireCardDrag,
  wireHandles,
  wireStickies,
} from './shell/cards.js';
import {
  populateModelChoice,
  wireComposer,
  wirePackUpdate,
  wirePaste,
  wireRailToggle,
  wireReadingToggle,
  wireTitle,
} from './shell/chrome.js';
import { ask, cardsEl, composer, el, gateEl, titleInput } from './shell/dom.js';
import { startLearning, tutorTurn, wireExercise, wireLearn } from './shell/learn.js';
import {
  heightOf,
  keepAsSticky,
  linked,
  reload,
  renderBoard,
  rpc,
  setMode,
  state,
  submit,
  toast,
  viewport,
} from './shell/state.js';

viewport.attach();

/** Doc 09 section 3's highlight and block investigate popovers, as one. */
const popover = new AnchorPopover(
  {
    root: el<HTMLElement>('anchor-pop'),
    label: document.querySelector('#anchor-pop .anchor-label') as HTMLElement,
    ask: el<HTMLButtonElement>('anchor-ask'),
    note: el<HTMLButtonElement>('anchor-note'),
    compose: document.querySelector('#anchor-pop .compose') as HTMLFormElement,
    question: el<HTMLInputElement>('anchor-question'),
    cancel: el<HTMLButtonElement>('anchor-cancel'),
  },
  (target, question) => {
    window.getSelection()?.removeAllRanges();
    void submit(question, {
      parentCardId: target.cardId,
      ...(target.anchorText ? { anchorText: target.anchorText } : {}),
      ...(target.anchorBlockRef ? { anchorBlockRef: target.anchorBlockRef } : {}),
    });
  },
  (target) => {
    window.getSelection()?.removeAllRanges();
    void keepAsSticky(target.cardId, target.anchorText ?? target.label);
  },
);
popover.attach();

/**
 * The rail and the four pages. Doc 11 section 5.
 *
 * The page layer covers the canvas rather than replacing it, so the board keeps
 * its camera, its cards and any in flight run while a page is open.
 */
const router = new Router(
  {
    rail: el<HTMLElement>('rail'),
    board: el<HTMLElement>('main'),
    page: el<HTMLElement>('page'),
    title: el<HTMLElement>('page-title'),
    tools: el<HTMLElement>('page-tools'),
    body: el<HTMLElement>('page-body'),
    flagCount: el<HTMLElement>('rail-flags'),
  },
  rpc,
  {
    openBoard: async (id) => {
      state.boardId = id;
      state.lastEventIndex = 0;
      await reload();
      await router.go('board');
    },
    createBoard: async () => {
      const { board_id } = await rpc.createBoard();
      state.boardId = board_id;
      state.lastEventIndex = 0;
      await reload();
      await router.go('board');
    },
    ask: (question) => void submit(question),
    // Doc 17 section 6's node verbs. The panel state lives in the shell, so the
    // Map asks for a lesson rather than starting one: a router that called
    // `learn.start` itself would leave the shell showing a board with a session
    // it does not know about.
    startLesson: async (topic, check) => {
      const { board_id } = await rpc.createBoard(topic);
      state.boardId = board_id;
      state.lastEventIndex = 0;
      await reload();
      await router.go('board');
      await startLearning(topic);
      if (check) await tutorTurn(() => rpc.askCheck(board_id));
    },
    toast: (message, level) => toast(message, level),
    keySaved: () => void populateModelChoice(),
    finishSetup: async () => {
      // A board to land on. Setup runs on a profile with none, and arriving at
      // an empty canvas with no board is the one state the composer cannot ask
      // from, so the finish makes one rather than leaving the person somewhere
      // that looks ready and is not.
      if (!state.boardId) {
        const { boards } = await rpc.listBoards();
        state.boardId = boards[0]?.id ?? (await rpc.createBoard()).board_id;
      }
      await reload();
      await router.go('board');
      ask.focus();
    },
  },
);
router.attach();

linked.popover = popover;
linked.router = router;

// Doc 17 section 2.2: a card the learner dwelt on is a card they read, and the
// concepts it links move from unseen to exposed. Started once, for the life of
// the shell: the watcher rebuilds what it observes as the board redraws.
watchExposure({
  cards: cardsEl,
  report: (cardId) => {
    if (!state.boardId) return;
    // Nothing is shown and nothing is retried. Exposure is a side note about
    // reading, and a toast about one failing would be the app talking about
    // itself while somebody is trying to read.
    void rpc.cardViewed(state.boardId, cardId).catch(() => {});
  },
});

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

  wireRailToggle();
  wireReadingToggle();
  wireExercise();
  wirePaste();
  wireLearn();
  wireComposer();
  wireCardActions();
  wireCardDrag();
  wireStickies();
  wireHandles();
  wireBranching();
  wireBuildTrail();
  wireTitle();
  wirePackUpdate();

  // No core behind the page: a plain browser can still see the canvas render,
  // which is what the fixture is for, but it cannot ask anything.
  if (!rpc.connected) {
    setMode(COPY.modeOffline, 'offline');
    const fixture = makeBoard(Number(params.get('cards') ?? '6'));
    state.board = fixture;
    titleInput.value = fixture.title;
    renderBoard(fixture);
    viewport.fit(boundsOf(fixture.cards, heightOf));
    composer.classList.add('disabled');
    ask.placeholder = COPY.askOffline;
    ask.disabled = true;
    return;
  }

  try {
    // Doc 11 section 6's First run. Asked of the core rather than inferred from
    // whether a board exists: a person who trashed their only board has not
    // become a new user, and a shell that decided this for itself would show
    // them the setup screen again.
    const first = await rpc.firstRun();
    setMode(COPY.modeLive, 'live');
    // The composer's model control fills from the profile, and nothing below
    // waits on it: the board is usable on auto while the options arrive.
    void populateModelChoice();
    if (first.needs_setup) {
      await router.go('setup');
      return;
    }

    const { boards } = await rpc.listBoards();
    state.boardId = boards[0]?.id ?? (await rpc.createBoard()).board_id;
    await reload();
    void router.refreshFlagCount();
    ask.focus();
  } catch (e) {
    const message = e instanceof RpcError ? e.message : COPY.coreSilent;
    setMode(COPY.modeOffline, 'offline');
    toast(message, 'error');
  }
}

void boot();
