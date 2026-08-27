/**
 * The Tutor panel. Doc 14 section 3.9.
 *
 * "Right dock, 320 px: log of tutor lines, learner picks, feedback chips with
 * olive for right and amber for wrong, tappable options, a text input. Stage
 * label in the header."
 *
 * The log is built from what the session holds rather than kept beside it, so a
 * panel reopened tomorrow shows the same conversation. Doc 14 section 3.9 also
 * says closing the panel ends the session and the board keeps everything, which
 * is why nothing here owns state the board does not.
 */

import { esc } from '../canvas/visual.js';
import type { LearnSession, TutorTurn } from '../rpc.js';
import { COPY } from '../strings.js';

/** One thing the panel is showing right now, on top of the session's history. */
export interface TutorState {
  turn: TutorTurn | null;
  /** Set once a check has been answered, so the feedback chip can say which. */
  feedback: { correct: boolean; explanation: string; remedy?: string } | null;
  busy: boolean;
}

function line(who: 'tutor' | 'learner', text: string): string {
  return `<li class="say ${who}">${esc(text)}</li>`;
}

/**
 * Doc 14 section 3.4: intake is tappable options and no free text.
 *
 * A question the session already holds an answer to is dropped rather than
 * re-offered. The log above it already shows the pair, so leaving the options
 * up would invite the learner to answer the same question twice and record two
 * different answers to it.
 */
function intakeHTML(turn: TutorTurn, session: LearnSession): string {
  const remaining = unanswered(turn, session);
  if (remaining.length === 0) return '';
  const rows = remaining
    .map(
      (q) =>
        `<li class="ask" data-q="${esc(q.q)}">` +
        `<p>${esc(q.q)}</p>` +
        `<div class="opts">${q.options
          .map((o) => `<button data-intake="${esc(o)}">${esc(o)}</button>`)
          .join('')}</div>` +
        `</li>`,
    )
    .join('');
  // Doc 14 section 3.4: "the learner may skip intake with just build it".
  return `${rows}<li class="ask"><div class="opts"><button data-learn-act="build">${COPY.learnSkipIntake}</button></div></li>`;
}

/**
 * The turn's questions the session has no answer for yet.
 *
 * Exported because the caller needs the same count to know when intake is over,
 * and two implementations of "is intake done" would drift apart.
 */
export function unanswered(
  turn: TutorTurn | null,
  session: LearnSession | null,
): { q: string; options: string[] }[] {
  const answered = new Set((session?.intake ?? []).map((a) => a.q));
  return (turn?.questions ?? []).filter((q) => !answered.has(q.q));
}

function planHTML(turn: TutorTurn): string {
  const plan = turn.plan;
  if (!plan) return '';
  return (
    `<li class="plan">` +
    `<p class="head">${esc(plan.title)}</p>` +
    `<ol>${plan.cards
      .map((c) => `<li><b>${esc(c.question)}</b><span>${esc(c.why)}</span></li>`)
      .join('')}</ol>` +
    `<button id="learn-open-plan" class="primary">${COPY.learnBuild}</button>` +
    `</li>`
  );
}

function checkHTML(turn: TutorTurn, state: TutorState): string {
  const check = turn.check;
  if (!check?.item) return '';
  const item = check.item;

  const options = item.options
    .map((o) => {
      // Doc 14 section 3.9: olive for right, amber for wrong, and only once the
      // learner has answered. A mark before that answers the question.
      let mark = '';
      if (state.feedback) {
        if (o.id === item.answer_id) mark = ' right';
        else mark = ' spent';
      }
      return (
        `<button class="opt${mark}" data-check-pick="${esc(o.id)}" ` +
        `${state.feedback ? 'disabled' : ''}>${esc(o.text)}</button>`
      );
    })
    .join('');

  // Doc 17 section 4's remedy, said as a sentence and offered as a choice. The
  // learner reads what a wrong answer suggests next and decides, which is doc
  // 14 section 3.7's rule that nothing happens on its own.
  const remedy = state.feedback?.remedy ? `<p class="remedy">${esc(state.feedback.remedy)}</p>` : '';
  const feedback = state.feedback
    ? `<p class="feedback ${state.feedback.correct ? 'right' : 'wrong'}">` +
      `${state.feedback.correct ? COPY.learnRight : COPY.learnWrong} ${esc(state.feedback.explanation)}</p>` +
      remedy +
      // Doc 14 section 3.4: then a choice. Never an automatic next step.
      `<div class="opts">` +
      `<button data-learn-act="next">${COPY.learnNext}</button>` +
      `<button data-learn-act="another">${COPY.learnAnother}</button>` +
      `<button data-learn-act="stop">${COPY.learnStop}</button>` +
      `</div>`
    : '';

  return `<li class="ask"><p>${esc(item.prompt)}</p><div class="opts">${options}</div>${feedback}</li>`;
}

/** Doc 14 section 3.9's header stage label, in words rather than a state name. */
export function stageLabel(session: LearnSession | null): string {
  switch (session?.status) {
    case 'intake':
      return COPY.learnStageIntake;
    case 'building':
      return COPY.learnStageBuilding;
    case 'reading':
      return COPY.learnStageReading;
    case 'checking':
      return COPY.learnStageChecking;
    case 'ended':
      return COPY.learnStageEnded;
    default:
      return COPY.learnStageIdle;
  }
}

export function tutorHTML(session: LearnSession | null, state: TutorState): string {
  if (!session) return `<p class="page-empty">${COPY.learnNone}</p>`;

  const log: string[] = [line('tutor', `${COPY.learnTopic} ${session.topic}`)];

  // The conversation so far, from the session rather than from memory.
  for (const answered of session.intake) {
    log.push(line('tutor', answered.q));
    log.push(line('learner', answered.a));
  }
  for (const check of session.checks) {
    log.push(line('learner', check.correct ? COPY.learnGotItRight : COPY.learnGotItWrong));
  }

  const turn = state.turn;
  const live = turn ? intakeHTML(turn, session) + planHTML(turn) + checkHTML(turn, state) : '';
  const reply = turn?.reply ? line('tutor', turn.reply) : '';

  // Doc 14 section 3.8: a provider failure says so and the session pauses.
  const caveats = (turn?.caveats ?? [])
    .map((c) => `<li class="say caveat">${esc(c)}</li>`)
    .join('');

  return (
    `<ul class="tutor-log">${log.join('')}${reply}${live}${caveats}</ul>` +
    (state.busy ? `<p class="page-empty">${COPY.learnThinking}</p>` : '') +
    `<form id="learn-say" class="tutor-say">` +
    `<input id="learn-message" aria-label="${COPY.learnAsk}" placeholder="${COPY.learnAsk}" autocomplete="off" />` +
    `</form>`
  );
}
