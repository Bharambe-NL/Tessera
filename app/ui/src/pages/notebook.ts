/**
 * Notebook. Doc 16 section 3.4.
 *
 * "A chat layout over the vault rather than a new engine: each question runs
 * the normal pipeline at deep depth with retrievers restricted to the vault and
 * the profile's own cards."
 *
 * What this view adds over the board is the grounding state. Doc 16 section 2.1
 * adopts the assessed package's one good idea about trust: an answer that found
 * nothing in your sources says so, visibly, and never quietly answers anyway.
 * So every turn wears one of three states, and the ungrounded one is the whole
 * reason the view exists rather than a failure of it.
 */

import { esc } from '../canvas/visual.js';
import type { NotebookSession, NotebookTurn } from '../rpc.js';
import { COPY } from '../strings.js';
import { emptyState } from './shared.js';

export interface NotebookState {
  /** The session being read, or `null` before one is opened. */
  session: NotebookSession | null;
  /** A question in flight, so the composer can say so. */
  asking: boolean;
}

export function notebookToolsHTML(state: NotebookState): string {
  if (!state.session) return '';
  return (
    `<div class="seg">` +
    `<button data-notebook-act="new">${COPY.notebookNew}</button>` +
    `</div>`
  );
}

/** Doc 16 section 3.4's three states, as the chip a person reads. */
function groundingChip(turn: NotebookTurn): string {
  switch (turn.grounding) {
    case 'grounded':
      return `<span class="chip grounded">${COPY.notebookGrounded}</span>`;
    case 'partly_grounded':
      return `<span class="chip partly">${COPY.notebookPartly}</span>`;
    case 'ungrounded':
      return `<span class="chip ungrounded">${COPY.notebookUngrounded}</span>`;
    default:
      return '';
  }
}

function sources(turn: NotebookTurn): string {
  if (turn.citations.length === 0) return '';
  return (
    `<ul class="notebook-sources">` +
    turn.citations
      .map(
        (c) =>
          `<li><span class="ord">${c.ordinal}</span> ${esc(c.source_title)}` +
          `<span class="chip">${esc(c.source_class)}</span></li>`,
      )
      .join('') +
    `</ul>`
  );
}

function turnHTML(turn: NotebookTurn): string {
  const answered = turn.status === 'done' || turn.status === 'flagged';
  return (
    `<li class="turn" data-card="${esc(turn.card_id)}">` +
    `<p class="asked">${esc(turn.question)}</p>` +
    `<div class="said">` +
    `<div class="line">${groundingChip(turn)}` +
    (turn.page_id ? `<span class="chip page">${COPY.savedAsPage}</span>` : '') +
    `</div>` +
    `<p class="answer">${esc(turn.answer ?? COPY.notebookThinking)}</p>` +
    sources(turn) +
    (turn.grounding === 'ungrounded'
      ? `<p class="page-note">${COPY.notebookUngroundedNote}</p>` +
        // Doc 16 section 3.4's one click way out, live now that doc 05 section
        // 8.1's web retriever exists. A profile with no web source set up is
        // told so when it is pressed, on the page that can fix it.
        `<button data-notebook-act="search-web">${COPY.notebookSearchWeb}</button>`
      : '') +
    (answered
      ? `<div class="verbs">` +
        (turn.page_id
          ? ''
          : `<button data-notebook-act="save">${COPY.saveAsPage}</button>`) +
        `<button data-notebook-act="open-board">${COPY.notebookOpenOnBoard}</button>` +
        `</div>`
      : '') +
    `</div></li>`
  );
}

export function notebookHTML(state: NotebookState): string {
  if (!state.session) {
    return (
      emptyState(COPY.notebookEmpty) +
      `<div class="setup-acts"><button class="primary" data-notebook-act="new">` +
      `${COPY.notebookStart}</button></div>`
    );
  }

  const turns = state.session.turns;
  return (
    (turns.length === 0
      ? emptyState(COPY.notebookNoTurns)
      : `<ul class="notebook">${turns.map(turnHTML).join('')}</ul>`) +
    `<form id="notebook-ask" class="notebook-ask">` +
    `<input id="notebook-question" placeholder="${COPY.notebookPlaceholder}" ` +
    `aria-label="${COPY.notebookPlaceholder}" autocomplete="off"${state.asking ? ' disabled' : ''} />` +
    `<button type="submit" class="primary"${state.asking ? ' disabled' : ''}>` +
    `${state.asking ? COPY.notebookAsking : COPY.notebookAsk}</button>` +
    `</form>`
  );
}
