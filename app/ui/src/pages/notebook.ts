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
import { button } from '../ui/button.js';
import { chip } from '../ui/chip.js';
import { emptyState } from './shared.js';

export interface NotebookState {
  /** The session being read, or `null` before one is opened. */
  session: NotebookSession | null;
  /** A question in flight, so the composer can say so. */
  asking: boolean;
}

export function notebookToolsHTML(state: NotebookState): string {
  if (!state.session) return '';
  return button(COPY.notebookNew, { data: { 'notebook-act': 'new' } });
}

/** Doc 16 section 3.4's three states, as the chip a person reads. */
function groundingChip(turn: NotebookTurn): string {
  switch (turn.grounding) {
    case 'grounded':
      return chip(COPY.notebookGrounded, { classes: 'grounded' });
    case 'partly_grounded':
      return chip(COPY.notebookPartly, { classes: 'partly' });
    case 'ungrounded':
      return chip(COPY.notebookUngrounded, { classes: 'ungrounded' });
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
          chip(c.source_class) +
          `</li>`,
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
    (turn.page_id ? chip(COPY.savedAsPage, { classes: 'page' }) : '') +
    `</div>` +
    `<p class="answer">${esc(turn.answer ?? COPY.notebookThinking)}</p>` +
    sources(turn) +
    (turn.grounding === 'ungrounded'
      ? `<p class="page-note">${COPY.notebookUngroundedNote}</p>` +
        // Doc 16 section 3.4's one click way out, live now that doc 05 section
        // 8.1's web retriever exists. A profile with no web source set up is
        // told so when it is pressed, on the page that can fix it.
        button(COPY.notebookSearchWeb, { data: { 'notebook-act': 'search-web' } })
      : '') +
    (answered
      ? `<div class="verbs">` +
        (turn.page_id ? '' : button(COPY.saveAsPage, { data: { 'notebook-act': 'save' } })) +
        button(COPY.notebookOpenOnBoard, { data: { 'notebook-act': 'open-board' } }) +
        `</div>`
      : '') +
    `</div></li>`
  );
}

export function notebookHTML(state: NotebookState): string {
  if (!state.session) {
    return (
      emptyState(COPY.notebookEmpty) +
      `<div class="setup-acts">` +
      button(COPY.notebookStart, { variant: 'primary', data: { 'notebook-act': 'new' } }) +
      `</div>`
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
    button(state.asking ? COPY.notebookAsking : COPY.notebookAsk, {
      variant: 'primary',
      submit: true,
      disabled: state.asking,
    }) +
    `</form>`
  );
}
