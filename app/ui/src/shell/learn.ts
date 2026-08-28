/**
 * Learn mode and the exercise modal. Docs 08 and 14.
 *
 * The session lives in the core; this holds only what is on screen right now.
 * Doc 14 section 3.9 says closing the panel ends the session and the board keeps
 * everything, which is what makes that split the right one: nothing here is lost
 * that the board would miss.
 */

import { exerciseHTML, type ExerciseState } from '../pages/exercise.js';
import { stageLabel, tutorHTML, unanswered, type TutorState } from '../pages/tutor.js';
import { RpcError, type ExerciseRow, type LearnSession, type TutorTurn } from '../rpc.js';
import { COPY } from '../strings.js';
import {
  ask,
  el,
  exerciseBody,
  exerciseEl,
  learnToggle,
  tutorBody,
  tutorEl,
  tutorStage,
} from './dom.js';
import { rpc, state, submit, toast } from './state.js';

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
  if (!state.boardId) return;
  const id = state.boardId;
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

export function wireExercise(): void {
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

/** Whether Learn mode is on, readable by the composer without a cycle. */
export const learnState = { learning: false };

let session: LearnSession | null = null;
let tutorState: TutorState = { turn: null, feedback: null, busy: false };

function renderTutor(): void {
  tutorStage.textContent = stageLabel(session);
  tutorBody.innerHTML = tutorHTML(session, tutorState);
}

async function refreshSession(): Promise<void> {
  if (!state.boardId) return;
  try {
    session = (await rpc.learnSession(state.boardId)).session;
  } catch {
    // The panel keeps what it had; the next turn re-reads.
  }
  renderTutor();
}

/** Run one tutor call with the panel showing that it is working. */
export async function tutorTurn(work: () => Promise<{ turn: TutorTurn }>): Promise<void> {
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

export async function startLearning(topic: string): Promise<void> {
  if (!state.boardId || !topic.trim()) return;
  const id = state.boardId;
  learnState.learning = true;
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

export async function endLearning(): Promise<void> {
  if (!state.boardId || !session) {
    closeTutorPanel();
    return;
  }
  try {
    const summary = await rpc.endLearn(state.boardId);
    // Doc 17 section 5: the record is a page, and a page nobody is told about
    // is a file the learner finds by accident.
    const saved = summary.record_page_id ? ` ${COPY.learnRecordSaved}` : '';
    toast(`${COPY.learnEnded} ${summary.correct} ${COPY.builtOf} ${summary.checks}.${saved}`);
  } catch {
    // Ending is a courtesy to the record, not a gate on closing the panel.
  }
  closeTutorPanel();
}

function closeTutorPanel(): void {
  learnState.learning = false;
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
  if (!state.boardId) return;
  const id = state.boardId;
  await refreshSession();
  if (unanswered(tutorState.turn, session).length > 0) return;
  await tutorTurn(() => rpc.buildPlan(id));
}

export function wireLearn(): void {
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
    if (!target || !state.boardId) return;
    const id = state.boardId;

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
      // Doc 17 section 4: the ladder moves a concept, so grading is told which
      // one the check was about. The turn named it; the shell hands it back.
      const about = tutorState.turn.check.concept_id;
      void rpc
        .answerCheck(id, item, pick.dataset.checkPick ?? '', about ? [about] : [])
        .then((result) => {
          const remedy =
            result.remedy.kind === 'prerequisite'
              ? COPY.learnRemedyPrerequisite
              : result.remedy.kind === 'card'
                ? COPY.learnRemedyCard
                : undefined;
          tutorState.feedback = { correct: result.correct, explanation: item.explanation, remedy };
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
    if (!message || !state.boardId) return;
    const id = state.boardId;
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
  if (planned.length === 0 || !state.boardId) return;
  for (const card of planned) {
    await submit(card.question);
  }
  if (state.boardId) await tutorTurn(() => rpc.askCheck(state.boardId as string));
}
