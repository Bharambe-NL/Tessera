/**
 * Entry point.
 *
 * The canvas layer renders whatever `Card[]` it is handed, so this file is the
 * only place that knows a core exists. With `?gate=200` it runs the doc 12
 * phase 0 acceptance gate against a fixture board instead, which is why the
 * fixture and the RPC path both land here and nowhere else.
 */

import './styles/fonts.css';
import './styles/tokens.css';
import './styles/board.css';
import './styles/chrome.css';
import './styles/pages.css';

import { blockAnchor, selectionAnchor } from './canvas/anchor.js';
import { trailFor, trailHTML } from './canvas/built.js';
import { boundsOf, layout } from './canvas/layout.js';
import { readingHTML } from './canvas/reading.js';
import { drawEdges, measureHeights, renderCards, toggleFlags } from './canvas/render.js';
import type { Board, Depth } from './canvas/types.js';
import { ViewportHost } from './canvas/viewport.js';
import { makeBoard } from './perf/fixture.js';
import { formatResult, runGate } from './perf/gate.js';
import { exerciseHTML, type ExerciseState } from './pages/exercise.js';
import { Router } from './pages/router.js';
import { stageLabel, tutorHTML, unanswered, type TutorState } from './pages/tutor.js';
import { AnchorPopover } from './popover.js';
import {
  Rpc,
  RpcError,
  type AskAnchor,
  type ExerciseRow,
  type LearnSession,
  type Notification,
} from './rpc.js';
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
const readingEl = el<HTMLElement>('reading');
const readingToggle = el<HTMLButtonElement>('reading-toggle');
const packUpdate = el<HTMLButtonElement>('pack-update');
const exerciseEl = el<HTMLElement>('exercise');
const exerciseBody = el<HTMLElement>('ex-body');
const tutorEl = el<HTMLElement>('tutor');
const tutorBody = el<HTMLElement>('tutor-body');
const tutorStage = el<HTMLElement>('tutor-stage');
const learnToggle = el<HTMLButtonElement>('learn');

const rpc = new Rpc();
const viewport = new ViewportHost({ main, world, onSettled: () => void 0 });
viewport.attach();

/** Doc 09 section 3's highlight and block investigate popovers, as one. */
const popover = new AnchorPopover(
  {
    root: el<HTMLElement>('anchor-pop'),
    label: document.querySelector('#anchor-pop .anchor-label') as HTMLElement,
    ask: el<HTMLButtonElement>('anchor-ask'),
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
    board: main,
    page: el<HTMLElement>('page'),
    title: el<HTMLElement>('page-title'),
    tools: el<HTMLElement>('page-tools'),
    body: el<HTMLElement>('page-body'),
    flagCount: el<HTMLElement>('rail-flags'),
  },
  rpc,
  {
    openBoard: async (id) => {
      boardId = id;
      lastEventIndex = 0;
      await reload();
      await router.go('board');
    },
    createBoard: async () => {
      const { board_id } = await rpc.createBoard();
      boardId = board_id;
      lastEventIndex = 0;
      await reload();
      await router.go('board');
    },
    ask: (question) => void submit(question),
    toast: (message, level) => toast(message, level),
    finishSetup: async () => {
      // A board to land on. Setup runs on a profile with none, and arriving at
      // an empty canvas with no board is the one state the composer cannot ask
      // from, so the finish makes one rather than leaving the person somewhere
      // that looks ready and is not.
      if (!boardId) {
        const { boards } = await rpc.listBoards();
        boardId = boards[0]?.id ?? (await rpc.createBoard()).board_id;
      }
      await reload();
      await router.go('board');
      ask.focus();
    },
  },
);
router.attach();

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
/** Doc 11 section 10's list view alternative, when it is the one being read. */
let reading = false;

function renderReading(b: Board): void {
  readingEl.innerHTML = readingHTML(b);
}

function renderBoard(b: Board): void {
  if (reading) renderReading(b);
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
/** Set when a notification changed how many flags are open. */
let flagCountStale = false;

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
async function drainNotifications(allowReload = true): Promise<void> {
  if (!boardId) return;
  try {
    const { notifications, index } = await rpc.notifications(boardId, lastEventIndex);
    lastEventIndex = index;
    for (const n of notifications) applyNotification(n);
    if (flagCountStale) {
      flagCountStale = false;
      void router.refreshFlagCount();
    }
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
  // Every rect the popover was placed against is about to be replaced.
  popover.close();
  board = await rpc.getBoard(boardId);
  titleInput.value = board.title;
  showPackUpdate(board);
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
  // Doc 14 section 4: with Learn on, the composer names a topic rather than
  // asking a question, and the tutor interviews before anything is asked.
  if (learnToggle.getAttribute('aria-pressed') === 'true' && !learning) {
    void startLearning(question);
    return;
  }
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
/**
 * Fill one card's "How this was built" body from `board.history`.
 *
 * Read on open rather than on render: the disclosure is closed on most cards
 * most of the time, and the history is the whole board's log, so fetching it per
 * card per render would read the same hundreds of events once per card.
 */
async function fillBuildTrail(body: HTMLElement, cardId: string): Promise<void> {
  if (!boardId || body.dataset.filled === cardId) return;
  try {
    const { events } = await rpc.history(boardId);
    body.innerHTML = trailHTML(trailFor(cardId, events));
    body.dataset.filled = cardId;
  } catch {
    body.textContent = COPY.builtFailed;
  }
}

function wireBuildTrail(): void {
  // `toggle` does not bubble, so it is caught in the capture phase. One listener
  // for every disclosure on the board, present and future.
  cardsEl.addEventListener(
    'toggle',
    (e) => {
      const details = e.target as HTMLDetailsElement | null;
      if (!details?.open || !details.classList.contains('built')) return;
      const body = details.querySelector<HTMLElement>('.built-body');
      const cardId = body?.dataset.builtFor;
      if (body && cardId) void fillBuildTrail(body, cardId);
    },
    true,
  );
}

/**
 * Offer to branch from a selected span or a clicked block.
 *
 * `pointerup` rather than `selectionchange`, because a selection being dragged
 * changes on every frame and a popover that follows it is unusable.
 */
function wireBranching(): void {
  cardsEl.addEventListener('pointerup', () => {
    // After the browser has settled the selection this gesture produced.
    window.setTimeout(() => {
      const anchor = selectionAnchor();
      if (anchor) popover.show(anchor);
      else if (popover.anchored?.anchorText) popover.close();
    }, 0);
  });

  cardsEl.addEventListener('click', (e) => {
    const anchor = blockAnchor(e.target);
    if (!anchor) return;
    e.stopPropagation();
    popover.show(anchor);
  });

  // A click anywhere else puts the popover away, the way a menu behaves.
  document.addEventListener('pointerdown', (e) => {
    if (!popover.open) return;
    const target = e.target as HTMLElement | null;
    if (target?.closest('#anchor-pop')) return;
    if (target?.closest('.card .body')) return;
    popover.close();
  });
}

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
 * Doc 10 section 9: the board offers the update, and nothing takes it on the
 * board's behalf.
 *
 * The version the board pinned is on the button, because "update pack" without
 * it asks a person to accept a change they cannot see the size of.
 */
function showPackUpdate(board: Board): void {
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

function wirePackUpdate(): void {
  packUpdate.addEventListener('click', () => {
    if (!boardId) return;
    const id = boardId;
    packUpdate.disabled = true;
    void rpc
      .updateBoardPack(id)
      .then(async (report) => {
        if (report.updated) {
          const flagged = report.cards.filter((c) => c.status === 'flagged').length;
          toast(
            `${COPY.boardPackUpdated} ${report.pack_code} ${report.to_version}: ` +
              `${report.cards.length} cards, ${flagged} flagged`,
          );
        }
        await reload();
      })
      .catch((e: unknown) => {
        toast(e instanceof RpcError ? e.message : COPY.boardPackUpdateFailed, 'error');
      })
      .finally(() => {
        packUpdate.disabled = false;
      });
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

/** Doc 11 section 5: the rail is 56px collapsed and 240px open. */
/**
 * Swap the canvas for the same cards as a document.
 *
 * The canvas is hidden from assistive technology while the document is open and
 * the document is hidden while the canvas is, rather than both being present:
 * two copies of every card in the accessibility tree is worse than either one.
 */
function wireReadingToggle(): void {
  readingToggle.addEventListener('click', () => {
    reading = !reading;
    readingToggle.setAttribute('aria-pressed', String(reading));
    readingToggle.textContent = reading ? COPY.readingClose : COPY.readingOpen;
    readingEl.hidden = !reading;
    world.setAttribute('aria-hidden', String(reading));
    emptyState.hidden = reading || (board?.cards.length ?? 0) > 0;
    if (reading && board) {
      renderReading(board);
      readingEl.focus();
    }
  });
}

/**
 * The exercise, as a modal over the board. Doc 08 section 3's on demand trigger.
 *
 * State lives here rather than in the page module, because an exercise is a
 * thing you are part way through: a reader who has answered three of five and
 * clicks a card behind the modal should find their three still chosen.
 */
let exercise: ExerciseRow | null = null;
let exerciseState: ExerciseState = { answers: {}, graded: null, empty: 'idle' };

function renderExercise(): void {
  exerciseBody.innerHTML = exerciseHTML(exercise, exerciseState);
}

function closeExercise(): void {
  exerciseEl.hidden = true;
  exercise = null;
  exerciseState = { answers: {}, graded: null, empty: 'idle' };
}

async function openExercise(): Promise<void> {
  if (!boardId) return;
  const id = boardId;
  exercise = null;
  exerciseState = { answers: {}, graded: null, empty: 'working' };
  exerciseEl.hidden = false;
  renderExercise();
  // The dialog takes focus, so Escape reaches it and a screen reader lands
  // inside rather than staying on the board behind it.
  exerciseEl.focus();

  try {
    const made = await rpc.makeExercise(id);
    // Doc 08 section 9 admits the exercise and names what was dropped, so the
    // reader is told rather than left to notice a short list.
    if (made.dropped > 0) toast(COPY.exerciseDropped, 'warn');
    if (made.exercise_id === null) {
      // Doc 08 section 10: the board had no card checked against a source. That
      // is an outcome with a reason, and it is not the same absence as a modal
      // that has not asked for one.
      exerciseState.empty = 'none_eligible';
    } else {
      const { exercises } = await rpc.exercises(id);
      exercise = exercises.find((e) => e.id === made.exercise_id) ?? null;
      if (!exercise) exerciseState.empty = 'failed';
    }
  } catch (e) {
    toast(e instanceof RpcError ? e.message : COPY.exerciseFailed, 'error');
    // An exercise that could not be generated shows its own empty state rather
    // than the last one, which would be a different board's questions.
    exercise = null;
    exerciseState.empty = 'failed';
  }
  renderExercise();
}

/**
 * Paste an image onto the board and read it. Doc 07 section A3.
 *
 * The paste is the whole gesture: doc 07 puts "Read this image" on an Image row
 * too, and that arrives with the Library's image surface. What a reader does
 * today is paste a screenshot of a table and want it read, so that is what this
 * does in one step.
 */
/**
 * Learn mode. Doc 14.
 *
 * The session lives in the core; this holds only what is on screen right now.
 * Doc 14 section 3.9 says closing the panel ends the session and the board keeps
 * everything, which is what makes that split the right one: nothing here is lost
 * that the board would miss.
 */
let learning = false;
let session: LearnSession | null = null;
let tutorState: TutorState = { turn: null, feedback: null, busy: false };

function renderTutor(): void {
  tutorStage.textContent = stageLabel(session);
  tutorBody.innerHTML = tutorHTML(session, tutorState);
}

async function refreshSession(): Promise<void> {
  if (!boardId) return;
  try {
    session = (await rpc.learnSession(boardId)).session;
  } catch {
    // The panel keeps what it had; the next turn re-reads.
  }
  renderTutor();
}

/** Run one tutor call with the panel showing that it is working. */
async function tutorTurn(work: () => Promise<{ turn: import('./rpc.js').TutorTurn }>): Promise<void> {
  tutorState.busy = true;
  renderTutor();
  try {
    const { turn } = await work();
    tutorState = { turn, feedback: null, busy: false };
  } catch (e) {
    // Doc 14 section 3.8: the panel says so and the session pauses. The board
    // remains usable, which is why nothing here touches it.
    toast(e instanceof RpcError ? e.message : COPY.learnFailed, 'error');
    tutorState.busy = false;
  }
  await refreshSession();
}

async function startLearning(topic: string): Promise<void> {
  if (!boardId || !topic.trim()) return;
  const id = boardId;
  learning = true;
  tutorEl.hidden = false;
  document.body.classList.add('learning');
  tutorState = { turn: null, feedback: null, busy: true };
  renderTutor();

  try {
    const { turn } = await rpc.startLearn(id, topic.trim());
    tutorState = { turn, feedback: null, busy: false };
  } catch (e) {
    toast(e instanceof RpcError ? e.message : COPY.learnFailed, 'error');
    tutorState.busy = false;
  }
  await refreshSession();
}

async function endLearning(): Promise<void> {
  if (!boardId || !session) {
    closeTutorPanel();
    return;
  }
  try {
    const summary = await rpc.endLearn(boardId);
    toast(`${COPY.learnEnded} ${summary.correct} ${COPY.builtOf} ${summary.checks}.`);
  } catch {
    // Ending is a courtesy to the record, not a gate on closing the panel.
  }
  closeTutorPanel();
}

function closeTutorPanel(): void {
  learning = false;
  session = null;
  tutorState = { turn: null, feedback: null, busy: false };
  tutorEl.hidden = true;
  document.body.classList.remove('learning');
  learnToggle.setAttribute('aria-pressed', 'false');
  ask.placeholder = COPY.askSomething;
}

/**
 * Record one intake answer, and ask for the plan once the last one is in.
 *
 * The plan is its own call because doc 14 section 3.4 lets the learner skip
 * intake, so building cannot be a side effect of finishing it. What that leaves
 * is a screen that has to notice when the questions run out: the first version
 * of this only refreshed the session, so a learner who answered every question
 * sat looking at the options they had already answered and the session never
 * left intake at all.
 */
async function answeredIntake(): Promise<void> {
  if (!boardId) return;
  const id = boardId;
  await refreshSession();
  if (unanswered(tutorState.turn, session).length > 0) return;
  await tutorTurn(() => rpc.buildPlan(id));
}

function wireLearn(): void {
  learnToggle.addEventListener('click', () => {
    const on = learnToggle.getAttribute('aria-pressed') !== 'true';
    learnToggle.setAttribute('aria-pressed', String(on));
    // Doc 14 section 4: the placeholder changes, because the composer is now
    // asking a different question.
    ask.placeholder = on ? COPY.learnPlaceholder : COPY.askSomething;
    if (!on) void endLearning();
  });

  el<HTMLButtonElement>('tutor-close').addEventListener('click', () => void endLearning());

  tutorEl.addEventListener('click', (e) => {
    const target = e.target as HTMLElement | null;
    if (!target || !boardId) return;
    const id = boardId;

    const picked = target.closest<HTMLElement>('[data-intake]');
    if (picked) {
      const question = picked.closest<HTMLElement>('.ask')?.dataset.q ?? '';
      void rpc
        .answerIntake(id, question, picked.dataset.intake ?? '')
        .then(answeredIntake)
        .catch(() => toast(COPY.learnFailed, 'error'));
      return;
    }

    if (target.closest('#learn-open-plan')) {
      void buildPlannedCards();
      return;
    }

    const pick = target.closest<HTMLElement>('[data-check-pick]');
    if (pick && tutorState.turn?.check?.item) {
      const item = tutorState.turn.check.item;
      void rpc
        .answerCheck(id, item, pick.dataset.checkPick ?? '')
        .then((result) => {
          tutorState.feedback = { correct: result.correct, explanation: item.explanation };
          renderTutor();
          void refreshSession();
        })
        .catch(() => toast(COPY.learnFailed, 'error'));
      return;
    }

    const verb = target.closest<HTMLElement>('[data-learn-act]')?.dataset.learnAct;
    if (verb === 'build') {
      void tutorTurn(() => rpc.buildPlan(id));
      return;
    }
    if (verb === 'stop') {
      void endLearning();
      return;
    }
    if (verb === 'another') {
      void tutorTurn(() => rpc.askCheck(id));
      return;
    }
    if (verb === 'next') {
      // Doc 14 section 3.4's opening: a follow-up on the target card, with the
      // reason recorded by whether the check went right.
      const check = tutorState.turn?.check;
      const question = tutorState.feedback?.correct ? check?.next_if_right : check?.next_if_wrong;
      if (question && check?.item) {
        void submit(question, { parentCardId: check.item.source_card_id });
      }
      tutorState.feedback = null;
      renderTutor();
    }
  });

  tutorEl.addEventListener('submit', (e) => {
    e.preventDefault();
    const input = el<HTMLInputElement>('learn-message');
    const message = input.value.trim();
    if (!message || !boardId) return;
    const id = boardId;
    input.value = '';
    void tutorTurn(() => rpc.sayToTutor(id, message));
  });
}

/**
 * Ask the questions the plan named. Doc 14 section 3.4: cards are requested in
 * parallel, and they are ordinary cards through the ordinary pipeline.
 *
 * Sequential here rather than parallel, because the core answers an ask
 * synchronously and firing five at once would queue them behind each other
 * anyway while the board showed five placeholders and no progress.
 */
async function buildPlannedCards(): Promise<void> {
  const planned = tutorState.turn?.plan?.cards ?? [];
  if (planned.length === 0 || !boardId) return;
  for (const card of planned) {
    await submit(card.question);
  }
  if (boardId) await tutorTurn(() => rpc.askCheck(boardId as string));
}

function wirePaste(): void {
  document.addEventListener('paste', (e) => {
    if (!boardId || !rpc.connected) return;

    const items = [...(e.clipboardData?.items ?? [])];
    const file = items
      .find((item) => item.kind === 'file' && item.type.startsWith('image/'))
      ?.getAsFile();
    if (!file) return;

    // A paste that also carries text, into a box that takes text, is text. The
    // first version of this checked only the focus, which reads correctly and
    // is wrong in practice: `boot` focuses the composer, so the composer always
    // has focus and no image would ever have been read at all.
    const into = document.activeElement;
    const typing = into instanceof HTMLInputElement || into instanceof HTMLTextAreaElement;
    const alsoText = items.some((item) => item.kind === 'string' && item.type === 'text/plain');
    if (typing && alsoText) return;

    e.preventDefault();
    void readPastedImage(file);
  });
}

async function readPastedImage(file: File): Promise<void> {
  if (!boardId) return;
  const id = boardId;

  // The size is read from the image itself rather than trusted from the
  // clipboard, because the packet tells a vision model what it is looking at.
  const bitmap = await createImageBitmap(file).catch(() => null);
  if (!bitmap) {
    toast(COPY.readFailed, 'error');
    return;
  }

  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  const data = btoa(binary);

  await whileRunning(async () => {
    const { image_id } = await rpc.addImage(id, data, file.type, bitmap.width, bitmap.height);
    return await rpc.read(id, image_id);
  }, COPY.readFailed);
}

function wireExercise(): void {
  el<HTMLButtonElement>('check').addEventListener('click', () => void openExercise());
  el<HTMLButtonElement>('ex-dismiss').addEventListener('click', closeExercise);

  exerciseEl.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeExercise();
  });

  exerciseEl.addEventListener('change', (e) => {
    const input = e.target as HTMLInputElement | null;
    if (input?.type !== 'radio' || exerciseState.graded) return;
    exerciseState.answers[input.name] = input.value;
    renderExercise();
  });

  exerciseEl.addEventListener('click', (e) => {
    const target = e.target as HTMLElement | null;
    if (!target) return;

    if (target.closest('#ex-close')) {
      closeExercise();
      return;
    }
    if (target.closest('#ex-submit') && exercise) {
      const id = exercise.id;
      void rpc
        .attempt(id, exerciseState.answers)
        .then((score) => {
          exerciseState.graded = { correct: score.correct, total: score.total };
          renderExercise();
        })
        .catch(() => toast(COPY.exerciseFailed, 'error'));
      return;
    }

    const verb = target.closest<HTMLElement>('[data-item-act]');
    if (!verb || !exercise) return;
    if (verb.dataset.itemAct === 'open') {
      // Doc 08 section 11: the item links to its source card, which is on the
      // board behind this modal.
      closeExercise();
      const card = document.getElementById(`card-${verb.dataset.card}`);
      card?.scrollIntoView({ block: 'center' });
      return;
    }
    if (verb.dataset.itemAct === 'report' && verb.dataset.item) {
      void rpc
        .reportItem(exercise.id, verb.dataset.item)
        .then(() => toast(COPY.exerciseReported))
        .catch(() => toast(COPY.exerciseFailed, 'error'));
    }
  });
}

function wireRailToggle(): void {
  const rail = el<HTMLElement>('rail');
  const toggle = el<HTMLButtonElement>('rail-toggle');
  toggle.addEventListener('click', () => {
    // The class goes on `body` rather than on the rail, because the rail's
    // width, the board's left padding and the page's left edge all read one
    // custom property and it has to be set where all three inherit it.
    const open = document.body.classList.toggle('rail-open');
    rail.classList.toggle('open', open);
    toggle.setAttribute('aria-expanded', String(open));
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

  wireRailToggle();
  wireReadingToggle();
  wireExercise();
  wirePaste();
  wireLearn();
  wireComposer();
  wireCardActions();
  wireBranching();
  wireBuildTrail();
  wireTitle();
  wirePackUpdate();

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
    // Doc 11 section 6's First run. Asked of the core rather than inferred from
    // whether a board exists: a person who trashed their only board has not
    // become a new user, and a shell that decided this for itself would show
    // them the setup screen again.
    const first = await rpc.firstRun();
    setMode(COPY.modeLive, 'live');
    if (first.needs_setup) {
      await router.go('setup');
      return;
    }

    const { boards } = await rpc.listBoards();
    boardId = boards[0]?.id ?? (await rpc.createBoard()).board_id;
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
