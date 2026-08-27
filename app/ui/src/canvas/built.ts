/**
 * "How this was built", rendered from events.
 *
 * Doc 09 section 4: "'How this was built' disclosure (plan, retrievers, model
 * calls, cost) rendered from events". Doc 09 section 12 makes it half of the
 * audit trail surface, the other half being board history.
 *
 * `board.history` has been registered on the core since M2 and no caller ever
 * called it, so the disclosure shell in `render.ts` opened onto an empty div.
 * Rendering from the log rather than from a summary field is the point: what
 * this shows is what happened, and there is no second place for it to disagree
 * with.
 */

import type { HistoryEntry } from '../rpc.js';
import { COPY } from '../strings.js';
import { esc } from './visual.js';

interface ModelCall {
  stage: string;
  model: string;
  input: number;
  output: number;
  latencyMs: number;
}

interface Retrieval {
  retriever: string;
  fetches: number;
  coverage: number | null;
}

/** What the events say about one card, reduced to what the disclosure shows. */
export interface BuildTrail {
  routedTo: string | null;
  subQuestions: number | null;
  plannedRetrievers: string[];
  retrievals: Retrieval[];
  calls: ModelCall[];
  inputTokens: number;
  outputTokens: number;
  latencyMs: number;
  /**
   * What the Verifier actually did, from `checks_run` and `verdict_counts`.
   *
   * Null until it has run. Saying "checked against its sources" on a fast card
   * was the first thing this disclosure got wrong: fast mode runs the
   * deterministic checks and cites nothing, so there were no sources to check
   * against and the row overstated what happened. What the event records is a
   * list of rules with an outcome each, so that is what this counts.
   */
  verify: VerifyRun | null;
  visual: 'produced' | 'declined' | null;
  /**
   * Prior cards this one was built on. Doc 01 section 4.4, doc 15 section 2.
   *
   * Read from `card.answered.v1` like everything else here rather than from the
   * card row, so the disclosure has one source and cannot disagree with itself.
   * Doc 12's walkthrough line 15 asks that a card building on a verified card
   * from another board be visible, and until now the core recorded it, the RPC
   * carried it, and no screen said a word.
   */
  buildsOn: { boardId: string; cardId: string }[];
}

interface VerifyRun {
  passed: number;
  failed: number;
  skipped: number;
  /** Citation verdicts by name, absent on a card that cited nothing. */
  verdicts: Record<string, number>;
}

function readVerify(payload: Record<string, unknown>): VerifyRun {
  const run: VerifyRun = { passed: 0, failed: 0, skipped: 0, verdicts: {} };

  for (const check of Array.isArray(payload.checks_run) ? payload.checks_run : []) {
    const outcome = (check as Record<string, unknown>)?.outcome;
    if (outcome === 'pass') run.passed += 1;
    else if (outcome === 'fail') run.failed += 1;
    else run.skipped += 1;
  }

  const counts = payload.verdict_counts;
  if (counts && typeof counts === 'object') {
    for (const [verdict, n] of Object.entries(counts as Record<string, unknown>)) {
      if (typeof n === 'number') run.verdicts[verdict] = n;
    }
  }
  return run;
}

function num(v: unknown): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : 0;
}

export function trailFor(cardId: string, events: HistoryEntry[]): BuildTrail {
  const trail: BuildTrail = {
    routedTo: null,
    subQuestions: null,
    plannedRetrievers: [],
    retrievals: [],
    calls: [],
    inputTokens: 0,
    outputTokens: 0,
    latencyMs: 0,
    verify: null,
    visual: null,
    buildsOn: [],
  };

  for (const event of events) {
    if (event.card_id !== cardId) continue;
    const p = (event.payload ?? {}) as Record<string, unknown>;

    switch (event.type) {
      case 'card.routed.v1':
        trail.routedTo = p.plan_required === true ? COPY.routedPlanned : COPY.routedDirect;
        break;
      case 'card.planned.v1':
        trail.subQuestions = num(p.sub_question_count);
        trail.plannedRetrievers = Array.isArray(p.retriever_ids) ? p.retriever_ids.map(String) : [];
        break;
      case 'retrieval.completed.v1':
        trail.retrievals.push({
          retriever: String(p.retriever_id ?? 'sources'),
          fetches: num(p.fetches),
          coverage: typeof p.coverage === 'number' ? p.coverage : null,
        });
        break;
      case 'model.call.v1':
        trail.calls.push({
          stage: String(p.stage ?? 'unknown'),
          model: String(p.model ?? 'unknown'),
          input: num(p.input_tokens),
          output: num(p.output_tokens),
          latencyMs: num(p.latency_ms),
        });
        trail.inputTokens += num(p.input_tokens);
        trail.outputTokens += num(p.output_tokens);
        trail.latencyMs += num(p.latency_ms);
        break;
      case 'visual.produced.v1':
        trail.visual = 'produced';
        break;
      case 'visual.declined.v1':
        trail.visual = 'declined';
        break;
      case 'verify.completed.v1':
        trail.verify = readVerify(p);
        break;
      case 'card.answered.v1':
        for (const prior of Array.isArray(p.builds_on) ? p.builds_on : []) {
          const entry = prior as Record<string, unknown>;
          const boardId = typeof entry.board_id === 'string' ? entry.board_id : '';
          const cardId = typeof entry.card_id === 'string' ? entry.card_id : '';
          if (boardId && cardId) trail.buildsOn.push({ boardId, cardId });
        }
        break;
    }
  }

  return trail;
}

function row(term: string, detail: string): string {
  return `<div class="built-row"><dt>${esc(term)}</dt><dd>${esc(detail)}</dd></div>`;
}

/** What the Verifier ran, counted rather than characterised. */
function verifyDetail(run: VerifyRun): string {
  const parts: string[] = [];
  if (run.passed) parts.push(`${run.passed} ${COPY.builtRulesPassed}`);
  if (run.failed) parts.push(`${run.failed} ${COPY.builtRulesFlagged}`);
  if (run.skipped) parts.push(`${run.skipped} ${COPY.builtRulesSkipped}`);

  const supported = run.verdicts.supported ?? 0;
  const cited = Object.values(run.verdicts).reduce((a, b) => a + b, 0);
  // Only when there were citations. A fast card cites nothing, and "0 of 0
  // citations supported" reads as a failure rather than as an absence.
  if (cited > 0) parts.push(`${supported} ${COPY.builtOf} ${cited} ${COPY.builtCitationsSupported}`);

  return parts.length ? parts.join(', ') : COPY.builtNoChecks;
}

/**
 * Render the trail.
 *
 * A card whose events say nothing says so, rather than showing an empty list
 * that reads as "nothing happened".
 */
export function trailHTML(trail: BuildTrail): string {
  if (trail.calls.length === 0 && trail.retrievals.length === 0 && trail.routedTo === null) {
    return `<p class="built-empty">${COPY.builtNothing}</p>`;
  }

  const rows: string[] = [];
  if (trail.routedTo) rows.push(row(COPY.builtRouted, trail.routedTo));

  if (trail.subQuestions !== null) {
    const retrievers = trail.plannedRetrievers.join(', ');
    rows.push(
      row(
        COPY.builtPlanned,
        retrievers
          ? `${trail.subQuestions} ${COPY.builtSubQuestions}, ${retrievers}`
          : `${trail.subQuestions} ${COPY.builtSubQuestions}`,
      ),
    );
  }

  for (const r of trail.retrievals) {
    const coverage = r.coverage === null ? '' : `, ${COPY.builtCoverage} ${r.coverage.toFixed(2)}`;
    rows.push(row(r.retriever, `${r.fetches} ${COPY.builtPassages}${coverage}`));
  }

  if (trail.visual) {
    rows.push(row(COPY.builtVisual, trail.visual === 'produced' ? COPY.builtDrawn : COPY.builtDeclined));
  }
  if (trail.verify) rows.push(row(COPY.builtVerified, verifyDetail(trail.verify)));

  // Doc 15 section 2's rule, said where a reader can act on it: a prior card is
  // context and the source it cited is the evidence, so this row names the
  // cards and never stands in for the citations below it.
  if (trail.buildsOn.length) {
    rows.push(
      row(
        COPY.builtBuildsOn,
        trail.buildsOn.map((p) => `${p.boardId}/${p.cardId}`).join(', '),
      ),
    );
  }

  for (const c of trail.calls) {
    rows.push(row(c.stage, `${c.model}, ${c.input + c.output} ${COPY.builtTokens}, ${c.latencyMs} ms`));
  }

  // Doc 09 section 4 names cost. Tokens are what the log records, so tokens are
  // what this states: a currency figure would need a price the core never saw.
  const total =
    `<div class="built-total">${trail.calls.length} ${COPY.builtCalls}, ` +
    `${trail.inputTokens + trail.outputTokens} ${COPY.builtTokens}, ${trail.latencyMs} ms</div>`;

  return `<dl class="built-rows">${rows.join('')}</dl>${total}`;
}
