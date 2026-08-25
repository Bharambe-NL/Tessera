# 14. Learn Mode and the Tutor Agent v0.1

Register: working. Depends on: 01, 06, 07, 08, 09. Adds one agent (Tutor) and one board mode (Learn). Amends 01 (Board.mode, LearnSession entity), 03 (Router reads the mode), 09 (Tutor panel), 12 (build phase 9b).

## 1. What Learn mode is

The tutor interviews the learner, builds a curated board from the answers, and then runs alongside the board: it checks understanding, opens the next card from what the learner got right or wrong, and answers questions about the board. The learner keeps every normal affordance (highlight, block investigate, follow-ups, ink, paste). The two threads, tutor and canvas, feed each other through the same cards and events.

Learn mode adds no new answer path. Curated cards are ordinary cards through Router, retrievers, Synthesizer, Visualizer, and Verifier. Check questions are Exercise items with a single-card scope. The tutor's only new job is choosing what to ask and what to open next.

## 2. Data model amendments

- `Board.mode`: enum `explore | learn`. Default explore.
- New entity **LearnSession**: `id, board_id, topic, intake: [{q, a}], plan: [{card_id, question, why, visual_hint, order}], checks: [{exercise_id, item_id, card_id, picked, correct, at}], opened: [{card_id, reason: right | wrong | asked}], status: intake | building | reading | checking | ended, mastery: json map concept_id to score`. One per board; a board can have several sessions over time.
- New events: `learn.started.v1`, `learn.intake_answered.v1`, `learn.planned.v1`, `learn.check_asked.v1`, `learn.check_answered.v1 { correct }`, `learn.card_opened.v1 { reason }`, `learn.ended.v1 { checks, correct }`.

## 3. Tutor agent

### 3.1 Purpose, scope, non-goals

Decides what to ask and what to open. Produces intake questions, a board plan, check items, next-card proposals, and short replies to learner questions grounded in the board. Never answers a topic question itself; when the learner asks one, the tutor proposes a card and the pipeline answers with citations.

Out of scope: retrieval; free-text grading; writing card content.

### 3.2 Position

Runs in a loop beside the board, triggered by session state changes. Reads the LearnSession, the done cards (question, answer, visual labels, citations), Concepts on the board, the doctrine's learning templates and audiences, and the Profile role. Writes LearnSession updates and Exercise rows; requests cards through the normal `card.requested.v1` with `requested_by: tutor`.

Substrate: session state machine, packet, schemas, the loop. Doctrine: intake question templates per domain, curriculum shapes (foundation, mechanism, landscape, edge cases), mastery thresholds, audience phrasing.

### 3.3 Trigger

`learn.started.v1` (composer with Learn on); every intake answer; all planned cards reaching done; every check answered; every learner message in the panel; `card.answered.v1` for cards the tutor opened.

### 3.4 Session state machine

```
intake ──► building ──► reading ──► checking ──► (opening ──► reading) | checking | ended
```

- intake: 2 or 3 tappable questions; the learner may skip with "just build it".
- building: plan of 3 to 5 cards, ordered foundation to detail, each with a visual hint chosen for variety; cards requested in parallel; board title set from the plan unless the user named it.
- reading: wait until every planned card is done or flagged; a blocked card is replaced by a rephrased request once.
- checking: one Exercise item scoped to one card; the learner answers; feedback with the card's citations; then a choice: open the proposed next card (deeper if right, remedial if wrong), another check, or stop.
- opening: a follow-up card on the target card, reason recorded; back to reading.
- ended: summary with checks and mastery per concept; the board stays in explore mode with the session attached.

At any time the learner can type in the panel: the tutor replies from board content only and may propose a card.

### 3.5 Packets and outputs (abridged)

Intake output: `{questions: [{q, options[3]}]}`. Plan output: `{title, cards: [{question, why, visual_hint}]}`. Check output: an Exercise item (08 schema) plus `{next_if_right, next_if_wrong}` as follow-up questions on the same card. Reply output: `{reply, open: question | null}`.

Harness rules: check items must pass the Exercise traceability and distractor checks; `next_*` questions must reference the target card's entities (deterministic overlap check); the tutor may request at most one card per turn and at most 8 per session without the learner choosing "another"; no reply may contain a citation marker (the tutor cites nothing; cards do).

### 3.6 Mastery

Per concept linked to the target card: +1 on a correct check, -1 on a wrong one, floored at 0. The tutor prefers checks on concepts with the lowest mastery and proposes remedial cards on concepts that went wrong twice. Mastery is shown at session end and in the Library concept detail ("checked 3 times, 2 right").

### 3.7 Confidence and admit

Always admitted; the learner sees every decision as a choice, never as an automatic action. Cards opened by the tutor go through the Verifier like any other.

### 3.8 Failures

Intake or plan schema violation: retry once, then a doctrine default template (three cards: what it is, how it works, who is involved). Check violation: retry once, then skip to a reply asking the learner what they want to open. Provider failure: the panel says so and the session pauses; the board remains usable.

### 3.9 Review surface

The Tutor panel (right dock, 320 px): log of tutor lines, learner picks, feedback chips with olive for right and amber for wrong, tappable options, a text input. Stage label in the header. Closing the panel ends the session; the board keeps everything.

### 3.10 Eval

On the synthetic corpus: planned cards cover the topic's required facts at 0.85 (a learner who reads all planned cards meets the deep recall target); check item traceability 1.00; remedial card relevance (the wrong-path card cites the concept that was missed) 0.90; session cost within 6 cards' worth of deep calls; tutor replies contain no claim absent from the board 1.00.

## 4. UX amendments to 09

- Composer: a Learn toggle left of the depth selector. Placeholder becomes "What do you want to learn?". Depth applies to the planned cards.
- Board: planned cards lay out in a grid rather than a row; tutor-opened follow-ups attach beneath their target. The card header shows a small "tutor" chip on cards the tutor opened.
- Home: boards in learn mode show mastery as a fraction.
- Library: concept detail shows check history.

## 5. Build prompt amendment

Phase 9b, after Exercise: Tutor agent, LearnSession, panel, events. Acceptance: a full session on a synthetic topic runs intake, plan, three checks, one remedial card, and ends with a mastery map; every step appears in board history.

## 6. Open questions

1. Should the tutor be able to schedule a "come back tomorrow" check (spaced repetition) as an in-app reminder? Proposal: v1.1, since it needs a scheduler and a notification surface.
2. Whether intake should reuse the Profile's role to skip the background question. Proposal: yes, ask only goal and the topic fork when the role is set.
