/**
 * The exercise, as a modal over the board. Doc 11 section 6.
 *
 * Doc 08 section 11: "Review surface: none. Items link to their source card; a
 * wrong item is reported by the user from the card." So this asks, grades, and
 * offers one verb per item: report it. It never edits an item, because an
 * exercise is a record of what was generated and a corrected one would be a
 * different exercise.
 */

import { esc } from '../canvas/visual.js';
import type { ExerciseItem, ExerciseRow } from '../rpc.js';
import { COPY } from '../strings.js';

/** What the reader has chosen so far, and what the grader said. */
export interface ExerciseState {
  answers: Record<string, string>;
  graded: { correct: number; total: number } | null;
  /**
   * What to say when there is no exercise.
   *
   * Three absences, and they are not the same absence. `idle` is a modal that
   * has not asked for one; `working` is one in flight; `none_eligible` is doc 08
   * section 10's `no_eligible_cards`, which is the core reporting that the board
   * has no card checked against a source. Collapsing them showed "No exercise
   * yet" on a board that had just been told exactly why there was none.
   */
  empty: 'idle' | 'working' | 'none_eligible' | 'failed';
}

function option(item: ExerciseItem, o: { id: string; text: string }, state: ExerciseState): string {
  const chosen = state.answers[item.id] === o.id;
  // Doc 08 section 9 admits every item, so the mark appears only after grading:
  // showing it earlier would answer the question.
  let mark = '';
  if (state.graded) {
    if (o.id === item.answer_id) mark = ' right';
    else if (chosen) mark = ' wrong';
  }
  return (
    `<label class="opt${mark}${chosen ? ' chosen' : ''}">` +
    `<input type="radio" name="${esc(item.id)}" value="${esc(o.id)}" ` +
    `${chosen ? 'checked' : ''} ${state.graded ? 'disabled' : ''} />` +
    `<span>${esc(o.text)}</span>` +
    `</label>`
  );
}

function itemHTML(item: ExerciseItem, index: number, state: ExerciseState): string {
  const explanation = state.graded
    ? `<p class="explanation">${esc(item.explanation)}</p>` +
      // Doc 08 section 11: the item links to its source card.
      `<div class="verbs">` +
      `<button data-item-act="open" data-card="${esc(item.source_card_id)}">${COPY.exerciseOpenCard}</button>` +
      `<button data-item-act="report" data-item="${esc(item.id)}">${COPY.exerciseReport}</button>` +
      `</div>`
    : '';

  return (
    `<li class="ex-item" data-item="${esc(item.id)}">` +
    `<p class="q"><span class="n">${index + 1}</span>${esc(item.prompt)}</p>` +
    `<div class="opts">${item.options.map((o) => option(item, o, state)).join('')}</div>` +
    explanation +
    `</li>`
  );
}

export function exerciseHTML(exercise: ExerciseRow | null, state: ExerciseState): string {
  if (!exercise || exercise.items.length === 0) {
    const empty = exercise && exercise.items.length === 0 ? 'none_eligible' : state.empty;
    const message = {
      idle: COPY.exerciseNone,
      working: COPY.exerciseWorking,
      // Doc 08 section 10's `no_eligible_cards`, said in the reader's words.
      none_eligible: COPY.exerciseNothingToCheck,
      failed: COPY.exerciseFailed,
    }[empty];
    return `<p class="page-empty">${message}</p>`;
  }

  const answered = Object.keys(state.answers).length;
  const foot = state.graded
    ? `<p class="score">${COPY.exerciseScored} ${state.graded.correct} ${COPY.builtOf} ${state.graded.total}</p>` +
      `<button id="ex-close" class="primary">${COPY.exerciseDone}</button>`
    : `<button id="ex-submit" class="primary" ${answered === exercise.items.length ? '' : 'disabled'}>` +
      `${COPY.exerciseSubmit}</button>` +
      `<span class="progress">${answered} ${COPY.builtOf} ${exercise.items.length}</span>`;

  return (
    `<ol class="ex-items">${exercise.items.map((i, n) => itemHTML(i, n, state)).join('')}</ol>` +
    `<div class="ex-foot">${foot}</div>`
  );
}
