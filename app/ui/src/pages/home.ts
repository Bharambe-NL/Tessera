/**
 * Home. Doc 09 section 3: "Boards grid; open flag count per board; last
 * activity".
 *
 * Doc 09 open question 1, adopted by doc 11: Trash is a filter here rather than
 * a rail item, so the same grid renders both and the toolbar switches which.
 * `board.list` already returned `open_flags`, so the grid needed no new read.
 */

import { esc } from '../canvas/visual.js';
import type { BoardSummary } from '../rpc.js';
import { COPY } from '../strings.js';
import { ago, emptyState } from './shared.js';

export type HomeFilter = 'active' | 'trashed';

function card(board: BoardSummary, filter: HomeFilter): string {
  const flags =
    board.open_flags > 0
      ? `<span class="chip flag warn" title="${COPY.homeOpenFlags}">${board.open_flags}</span>`
      : '';
  // Doc 09 section 5: Remove on a board goes to Trash, and only a trashed board
  // can be purged. The verbs a row offers are the verbs its filter allows.
  const verbs =
    filter === 'active'
      ? `<button data-board-act="open" data-board="${esc(board.id)}">${COPY.homeOpen}</button>` +
        `<button data-board-act="trash" data-board="${esc(board.id)}">${COPY.homeTrash}</button>`
      : `<button data-board-act="restore" data-board="${esc(board.id)}">${COPY.homeRestore}</button>` +
        `<button class="danger" data-board-act="purge" data-board="${esc(board.id)}">${COPY.homePurge}</button>`;

  return (
    `<article class="board-card" data-board="${esc(board.id)}">` +
    `<header><h2>${esc(board.title)}</h2>${flags}</header>` +
    `<p class="meta">${board.cards} ${COPY.homeCards}, ${esc(ago(board.updated_at))}</p>` +
    `<div class="verbs">${verbs}</div>` +
    `</article>`
  );
}

export function homeHTML(boards: BoardSummary[], filter: HomeFilter): string {
  if (boards.length === 0) {
    return emptyState(filter === 'active' ? COPY.homeNoBoards : COPY.homeNoTrash);
  }
  return `<div class="board-grid">${boards.map((b) => card(b, filter)).join('')}</div>`;
}

/** The filter toggle and the create button, which live in the page header. */
export function homeToolsHTML(filter: HomeFilter): string {
  const on = (f: HomeFilter) => (f === filter ? ' class="on"' : '');
  return (
    `<div class="seg" role="group" aria-label="${COPY.homeFilterLabel}">` +
    `<button data-home-filter="active"${on('active')}>${COPY.homeActive}</button>` +
    `<button data-home-filter="trashed"${on('trashed')}>${COPY.homeTrashed}</button>` +
    `</div>` +
    `<button id="home-create" class="primary">${COPY.homeCreate}</button>`
  );
}
