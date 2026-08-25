# 08. Exercise Agent v0.1

Register: working. Depends on: 01, 06, 07.

## 1. Purpose, scope, non-goals

Generates a short exercise from cards that already exist, so a reader can check their understanding of a board. Every item traces to a card and that card's citations. It reads Visuals and answers only; it never retrieves and never asks the model for new facts.

Out of scope: grading free text; adaptive difficulty; anything that adds a claim to the board.

## 2. Position

Reads Cards (status done or flagged with only warn flags; blocked content is excluded), Visuals, Concepts linked to the board, the doctrine's exercise templates and audiences. Writes one Exercise. Attempts are recorded by the UI, no agent.

Substrate: item schema, traceability rule, template mechanics. Doctrine: templates (what a good question looks like per domain), audience phrasing.

## 3. Trigger

On demand from a board ("Check understanding") or a card. Optionally on `bundle imported` for a recipient who chose "learn this board".

## 4. Packet

```json
{
  "schema_version": "1.0", "run_id": "ulid", "board_id": "ulid",
  "scope": { "card_ids": ["ulid"] },
  "cards": [ { "card_id": "ulid", "question": "string", "answer": "string", "findings": [], "visual": { "type": "string", "block_index": [] }, "citations": [ { "n": 1, "source_title": "string" } ] } ],
  "concepts": [ { "concept_id": "ulid", "term": "string", "definition": "string", "audience_definitions": {} } ],
  "template": { "id": "string", "item_kinds": ["recall", "apply", "contrast", "trace"], "items_per_card_max": 2, "options": 4 },
  "audience_id": "string | null",
  "effort_budget": { "max_tokens": 2500, "max_items": 8 }
}
```

## 5. Output schema

```json
{
  "schema_version": "1.0", "agent_id": "exercise", "run_id": "ulid",
  "title": "string",
  "items": [ { "id": "string", "kind": "recall | apply | contrast | trace", "prompt": "string",
               "options": [ { "id": "a", "text": "string" } ], "answer_id": "a",
               "explanation": "string", "source_card_id": "ulid", "citation_ordinals": [1], "concept_ids": ["ulid"] } ],
  "confidence": 0.0, "caveats": ["string"]
}
```

Harness rules: `answer_id` in options; `source_card_id` in scope; the correct option's text appears (normalised) in the source card's answer, findings, or visual labels; `citation_ordinals` exist on that card; distractors must not be true statements from any other card in scope (deterministic check against the other cards' values); items within budget.

## 6. State machine

`received ──► selecting_cards ──► drafting ──► checking_traceability ──► checking_distractors ──► emitting ──► done`; retry once with the failing items named; items that still fail are dropped.

## 7. Events

`exercise.generated.v1 { board_id, item_count, kinds, audience_id }`, `attempt.recorded.v1` (from UI).

## 8. Pipeline

1. Select cards: exclude blocked content; prefer cards with confirmed concepts; cap by budget.
2. One medium call: for each card produce up to two items of the requested kinds. `recall` asks for a stated fact; `apply` gives a small scenario and asks which rule applies; `contrast` uses a table card's two columns; `trace` asks which source supports a claim (options are source titles from the board). Audience phrasing when set.
3. Traceability check, deterministic.
4. Distractor check, deterministic.

## 9. Confidence

Items passing both checks over items drafted. Always admitted; a low ratio adds a caveat "some items were dropped".

## 10. Failures

`schema_violation` retry then drop items; `no_eligible_cards` return empty with reason; provider failures fall back to the small alias once.

## 11. Review surface

None. Items link to their source card; a wrong item is reported by the user from the card, which records `exercise.item_reported.v1` for pack maintenance.

## 12. Eval

Traceability 1.00; distractor truth leakage 0; items per card within limit; synthetic boards: item answerable from the source card by a second model with only that card as context 0.95.

## 13. Performance

One medium call, 3 to 6 s.

## 14. Open questions

1. `trace` items reveal source titles as options, which can be a giveaway on boards with one source. Rule: only when the board has at least three distinct sources.

---

# 09. Review Queue and Board UX v0.1 (functional)

Register: working. Depends on: 01 to 08. Reference: `ai-native-ui-primitives.md` for the vocabulary (status chips, evidence panels, streaming states, review queues).

## 1. Purpose

Specify how agent output converges into one reviewer experience for a single user, and how the board surfaces exposed in the prototype map to the entities and events. This is functional, not visual; document 11 owns the look.

## 2. Reviewer mental modes

Single user, so four modes collapse to three:

1. **Working on a board.** The user asks, reads, branches. Flags appear inline and are handled in place. Nothing interrupts.
2. **Clearing flags.** The user opens the Flags queue and works through items across boards, deciding quickly. Bulk decisions matter here.
3. **Reopening an old board.** Stale sources have produced batch flags. The user wants a summary and a one click rerun.

## 3. Information architecture

Left rail, from the prototype, now bound to entities:

| Item | Shows | Entities |
|---|---|---|
| Home | Boards grid; open flag count per board; last activity | Board, Flag |
| Create | New board | Board |
| Flags | The queue (new) | Flag, Review, Card |
| Library | Sources and Concepts, filterable; saved highlights | Source, Concept, ConceptLink |
| Profile | Context, instructions, depth, model keys, retrievers, doctrine pack | Profile, DoctrinePack |
| Trash | Trashed boards, restore, purge | Board |

The board surface keeps: title, breadcrumb to parent board, tool strip (draggable), depth selector in the composer, cards with header badges (depth, model, confidence, flag count), per card follow-up input, highlight popover, block investigate popover, ink, notes, images, exercise entry ("Check understanding") in the toolbar.

## 4. Card anatomy, bound to state

- Header: title or anchor; depth badge; model alias on hover with "Rerun as…"; confidence dot (unchecked grey, under 0.5 amber, over 0.5 olive); flag count chip.
- Streaming states, derived from events: Routing (only if over 400 ms), Planning, Searching {retriever names}, Answering, Building the visual, Verifying. Research shows sub-question lines that tick off.
- Body: answer with citation superscripts; unsupported spans dotted underlined; findings; visual with clickable blocks; hidden blocks rendered as a placeholder with the flag reason; "Sources (n)" disclosure; "How this was built" disclosure (plan, retrievers, model calls, cost) rendered from events.
- Footer: follow-up input; "Check understanding" when the card is done.

## 5. Action vocabulary (standardised across artefact types)

Eight verbs, used identically for flags, cards, sources, concepts:

| Verb | On a flag | On a card | On a source | On a concept |
|---|---|---|---|---|
| Open | Go to the card, target highlighted | Go to board and pan | Open locator | Open library entry |
| Accept | Flag stands, content stays hidden | (n/a) | Confirm trust | Confirm |
| Dismiss | Reveal content, record decision | (n/a) | (n/a) | Reject proposal |
| Rerun | Rerun card with override | Rerun | Re-verify locator | (n/a) |
| Edit | Remove span or block, re-verify | Edit title, remove block | Edit title or issuer | Edit definition |
| Branch | (n/a) | Highlight or block branch | Ask about this source | Ask about this concept |
| Spin off | (n/a) | New board from block | (n/a) | New board from concept |
| Remove | (n/a) | Remove card and subtree | Remove (only if uncited) | Remove (only if unlinked) |

Every verb emits a user event and is undoable within the session except Remove on a board, which goes to Trash.

## 6. The Flags queue

Rows grouped by board, sorted by severity then age. Each row: severity chip, rule name, reason, card title, age, evidence preview (passage excerpt or stale date). Row actions: Open, Accept, Dismiss, Rerun. Bulk: select by rule ("dismiss all `length_and_format` on this board"), select by board ("rerun all stale on board X"). Bulk Dismiss requires a second click with the count shown. Bulk Accept needs no confirmation.

Batch flags: `verify_only` runs that flag several cards for the same stale source produce one queue row "Source X is stale, affects 5 cards" with a single Rerun that reruns all five.

Dismiss records the rule id; the Profile's doctrine page shows dismissals per rule over the last 30 days with a "disable rule" control. No automatic rule changes.

## 7. Urgency

No SLAs in a single user product. Ordering is severity, then age. A `block` flag on the board being viewed shows a persistent banner on the card until decided.

## 8. Notifications

In app only: a count on the Flags rail item; a toast when a `verify_only` run flags cards on the open board; a Profile notice when a key fails or a corpus update lands. No email, no OS notifications in v1.

## 9. Library

Two tabs. Sources: title, issuer, class, trust rank, cited on n cards, last verified, stale state; row actions Open, Rerun (re-verify), Edit. Concepts: term, status (proposed or confirmed), definition, audience definitions, linked cards; row actions Accept, Dismiss, Edit, Spin off. A concept detail lists every card and block that links to it across boards.

## 10. Audience lens

On a card header, "Explain for…" lists the pack's audiences. Choosing one reruns the Synthesizer's audience step only (no retrieval), producing a superseding Card version tagged with the audience; the chain menu switches between versions. On a board, "Explain board for…" queues the same for every done card.

## 11. Bundles

Export from the board title menu: a checklist of local document sources included, an "include history" toggle, then a file save. Import from Home: opens as a new board with the breadcrumb "forked from {name}", proposed concepts marked, imported runs marked read only.

## 12. Audit trail surface

"How this was built" on each card; "Board history" from the title menu, a chronological list rendered from events with filters by agent and by user actions; export as JSONL.

## 13. Admin views

The Profile is the admin view. Retrievers page (folders, corpora, tables, index status, parse errors, hook denials); Doctrine page (pack, version, rules with enable toggles and dismissal counts, audiences, rulings); Models page (keys in the keychain, aliases per stage, per provider spend this month from `Run.cost`).

## 14. Keyboard and accessibility commitments

Every verb reachable by keyboard; flag rows navigable with arrows; card focus ring; reduced motion respected; text contrast at 4.5:1; the tool strip's shortcuts documented in a hover.

## 15. Open questions

1. Whether the Flags rail item replaces Trash in the rail (six items is a lot). Proposal: Trash moves into Home as a filter.
2. "Explain board for…" cost: a 12 card board is 12 frontier calls. Show the estimate before running. Proposal: yes, with the estimate from `Run.cost` averages.
