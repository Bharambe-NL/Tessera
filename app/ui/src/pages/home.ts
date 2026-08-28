/**
 * Home. Doc 09 section 3: "Boards grid; open flag count per board; last
 * activity".
 *
 * Doc 09 open question 1, adopted by doc 11: Trash is a filter here rather than
 * a rail item, so the same grid renders both and the toolbar switches which.
 * `board.list` already returned `open_flags`, so the grid needed no new read.
 */

import { esc } from '../canvas/visual.js';
import type { BoardSummary, MissionSummary } from '../rpc.js';
import { COPY } from '../strings.js';
import { button } from '../ui/button.js';
import { chip } from '../ui/chip.js';
import { segmented } from '../ui/segmented.js';
import { ago, emptyState } from './shared.js';

export type HomeFilter = 'active' | 'trashed';

/**
 * Doc 17 section 6's last line: "Home shows, per mission, the fraction of
 * concepts at checked or better and the current frontier concept".
 *
 * A fraction rather than a percentage, because two of five is a thing a person
 * can hold and 40 percent of an unnamed total is not. The frontier is named,
 * not counted: what a learner wants from this line is what to do next.
 */
function missionHTML(summary: MissionSummary): string {
  if (!summary.mission || summary.concepts === 0) return '';
  const frontier = summary.frontier.length
    ? `${COPY.homeFrontier} ${esc(summary.frontier.join(', '))}`
    : COPY.homeNoFrontier;
  return (
    `<section class="mission">` +
    `<h2>${esc(summary.mission.statement)}</h2>` +
    `<p class="meta">${summary.checked_or_better} ${COPY.homeOf} ${summary.concepts} ` +
    `${COPY.homeChecked}. ${frontier}</p>` +
    `</section>`
  );
}

function card(board: BoardSummary, filter: HomeFilter): string {
  const flags =
    board.open_flags > 0
      ? chip(String(board.open_flags), { classes: 'flag warn', title: COPY.homeOpenFlags })
      : '';
  // Doc 09 section 5: Remove on a board goes to Trash, and only a trashed board
  // can be purged. The verbs a row offers are the verbs its filter allows.
  const verbs =
    filter === 'active'
      ? button(COPY.homeOpen, { data: { 'board-act': 'open', board: board.id } }) +
        button(COPY.homeTrash, { data: { 'board-act': 'trash', board: board.id } })
      : button(COPY.homeRestore, { data: { 'board-act': 'restore', board: board.id } }) +
        button(COPY.homePurge, { variant: 'danger', data: { 'board-act': 'purge', board: board.id } });

  return (
    `<article class="board-card" data-board="${esc(board.id)}">` +
    `<header><h2>${esc(board.title)}</h2>${flags}</header>` +
    `<p class="meta">${board.cards} ${COPY.homeCards}, ${esc(ago(board.updated_at))}</p>` +
    `<div class="verbs">${verbs}</div>` +
    `</article>`
  );
}

export function homeHTML(
  boards: BoardSummary[],
  filter: HomeFilter,
  mission: MissionSummary | null,
): string {
  // The mission line only belongs over the boards a learner is working on.
  const summary = filter === 'active' && mission ? missionHTML(mission) : '';
  if (boards.length === 0) {
    return summary + emptyState(filter === 'active' ? COPY.homeNoBoards : COPY.homeNoTrash);
  }
  return (
    summary + `<div class="board-grid">${boards.map((b) => card(b, filter)).join('')}</div>`
  );
}

/** The filter toggle and the create button, which live in the page header. */
export function homeToolsHTML(filter: HomeFilter): string {
  return (
    segmented(
      [
        { label: COPY.homeActive, on: filter === 'active', data: { 'home-filter': 'active' } },
        { label: COPY.homeTrashed, on: filter === 'trashed', data: { 'home-filter': 'trashed' } },
      ],
      { ariaLabel: COPY.homeFilterLabel },
    ) + button(COPY.homeCreate, { variant: 'primary', id: 'home-create' })
  );
}
