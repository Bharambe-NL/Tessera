# 06. Synthesizer and Visualizer Agents v0.1

Register: working. Depends on: 01 to 05. Load bearing patterns: 1, 2, 3, 4, 7; 6 (convergence, in research mode); 16 (audience vocabulary from doctrine).

Two agents in one document because they share inputs and run back to back; each has its own packet, schema, and failure taxonomy.

---

# Part A. Synthesizer

## A1. Purpose, scope, non-goals

Writes the prose answer and the key findings from retrieved passages, binding every sourced claim to a citation. In fast mode it writes from model knowledge with no citations and says so. It applies the audience lens when one is set. It answers descriptively when the advice flag is present.

Out of scope: retrieval; visual production; deciding whether its own claims are supported (the Verifier does); computing numbers.

## A2. Architectural position

After all retriever assignments for a run complete (or time out). Reads the plan, the passages, the ancestors, the doctrine's audience definitions and writing rules, the profile's standing instructions. Writes the Card answer, findings, and a provisional Citation set. The Visualizer reads its structured output next.

Substrate: packet, schema, citation marker discipline, audience parameter. Doctrine: audience vocabularies, writing rules (units, spelling, register), the advice rule wording, source hierarchy for conflict resolution.

## A3. Trigger

On demand after `retrieval.completed.v1` for every assignment, or directly after `card.routed.v1` in fast mode.

## A4. Task packet

```json
{
  "schema_version": "1.0", "run_id": "ulid", "card_id": "ulid",
  "mode": "fast | deep | research",
  "request": { "text": "string", "kind": "string", "anchor_text": "string | null" },
  "plan": { "sub_questions": [ { "sq_id": "string", "text": "string", "purpose": "string" } ], "constraints": { "must_include": [], "must_exclude": [], "answer_scope": "string", "audience_id": "string | null", "value_policy": "string", "stale_ancestor_citations": [] } } | null,
  "passages": [ { "passage_id": "ulid", "sq_id": "string", "text": "string", "source": { "title": "string", "class": "string", "issuer": "string", "trust_rank": 1, "published_at": "ISO8601 | null", "version_ref": "string | null" }, "score": 0.0 } ],
  "ancestors": [ { "question": "string", "answer_excerpt": "string", "stale": false } ],
  "flags": [ { "rule_id": "advice_request", "severity": "warn" } ],
  "audience": { "id": "string", "name": "string", "vocabulary_notes": "string", "avoid": ["string"] } | null,
  "writing_rules": { "units": "EUR", "spelling": "en-GB", "sentence_max_words": 28, "dashes": false },
  "standing_instructions": "string | null",
  "effort_budget": { "max_tokens": 3000, "answer_max_words": 180, "findings_max": 5 }
}
```

Passages arrive ordered by trust rank then score, capped by the plan budget. Passage text is the only material the model may cite from.

## A5. Output schema

```json
{
  "schema_version": "1.0", "agent_id": "synthesizer", "run_id": "ulid",
  "answer": "string, prose, citation markers as [n]",
  "findings": [ { "text": "string", "citations": [1] } ],
  "citations": [ { "n": 1, "passage_id": "ulid", "claim_span": { "start": 0, "end": 0 }, "binding": "answer | finding" } ],
  "conflicts": [ { "claim": "string", "readings": [ { "passage_id": "ulid", "value": "string" } ], "resolution": "higher_trust | later_date | presented_both" } ],
  "scope_statement": "string | null",
  "unsupported_statements": [ { "span": { "start": 0, "end": 0 }, "reason": "no_passage | model_knowledge" } ],
  "audience_applied": "string | null",
  "advice_handling": "none | reframed_descriptive",
  "structured_summary": { "entities": ["string"], "relations": [ { "from": "string", "to": "string", "kind": "string" } ], "values": [ { "label": "string", "value": "string", "unit": "string", "citation": 1 } ], "steps": ["string"], "groups": [ { "heading": "string", "items": ["string"] } ] },
  "confidence": 0.0, "caveats": ["string"]
}
```

Harness rules: every `[n]` in answer and findings has a citation with that n; every citation's `passage_id` is in the packet; `claim_span` offsets fall inside the answer; in deep and research modes a numeric value in `structured_summary.values` without a citation is a schema violation; in fast mode `citations` must be empty and `unsupported_statements` must cover the whole answer with reason `model_knowledge`. The `structured_summary` is the Visualizer's only input from the Synthesizer, so it must contain the entities and values the visual will show, each with its citation.

## A6. State machine

```
received ──► validating ──► drafting ──► binding_citations ──► reconciling_conflicts
   ──► applying_audience ──► summarising_structure ──► emitting ──► done
retry (once); failed on no_passages_in_deep_mode (research and deep only)
```

Drafting and binding are one model call with the frontier alias; reconciling is deterministic with a model tie break; audience application is a second call only when an audience is set; structure summarisation is part of the first call's JSON.

## A7. Events

`card.synthesized.v1 { card_id, run_id, mode, citation_count, conflict_count, unsupported_count, audience_id, advice_handling }`, `citation.bound.v1` per citation, `model.call.v1`, `schema.violation.v1`.

## A8. Reasoning pipeline

1. **Drafting.** The prompt contains the passages numbered by packet order, the request, the ancestors, the scope, the writing rules, the standing instructions. It asks for an answer of at most `answer_max_words` in which every claim drawn from a passage is followed by the passage number, and for the JSON above. The prompt states that anything without a passage number is treated as unsupported and will be flagged, so the model is better off omitting it.
2. **Binding.** Deterministic: markers are parsed, spans computed from the sentence containing the marker, `passage_id` looked up. Markers with no passage are removed and the sentence is listed as unsupported.
3. **Conflicts.** Deterministic detection when two cited passages give different values for the same labelled value in `structured_summary`. Resolution follows the doctrine: higher trust rank wins; equal rank, later `published_at` wins; otherwise both are presented and the conflict is recorded. The answer text is adjusted by a short second call only when the draft stated the losing value.
4. **Audience.** When set, a second call rewrites the answer for the audience using the vocabulary notes, keeping every citation marker in place. A deterministic check confirms the marker set is unchanged; if a marker was lost the rewrite is discarded and a caveat is added.
5. **Advice.** When the advice flag is present, the drafting prompt instructs a descriptive answer: what the rule says, what the options are, what each implies, with no recommendation. `advice_handling: reframed_descriptive`. The Verifier checks for recommendation language anyway.
6. **Fast mode.** No passages. One call, medium alias by default, shorter answer, the whole answer marked model knowledge. The card shows "Unverified" in the header.

Research mode adds a convergence step (Pattern 6): findings that appear in two or more sub-questions' passages are marked as convergent in a caveat free way (the `findings` array carries all citations), and findings supported by only one sub-question are listed after them.

## A9. Confidence

Deterministic: fraction of answer sentences with a supported citation (weight 0.5), fraction of `structured_summary.values` with a citation (0.2), no conflicts unresolved (0.15), no stale ancestor cited (0.15). Fast mode confidence is fixed at 0 and displayed as "Unverified". Always admitted; the Verifier decides what to hold.

## A10. Failure taxonomy

| Type | Recovery |
|---|---|
| `no_passages` in deep or research | Answer with `scope_statement` "No sources found for…" and an empty citation set, confidence 0, card marked "No sources"; never fall back to model knowledge silently. |
| `schema_violation` | Retry once with the violation; then fail the card with the draft preserved in the evidence bundle. |
| `marker_orphaned` | Deterministic removal, unsupported listing, continue. |
| `audience_rewrite_lost_citations` | Discard rewrite, caveat, continue. |
| `injection_detected` (passage text contains instructions addressed to the model and the draft follows them) | Deterministic detector on known patterns; drop the passage, redraft once, raise `flag.raised` rule `injection_suspected`. |
| `model_timeout`, `provider_unavailable` | Fallback alias once; then fail. |
| `unknown` | Evidence bundle; fail. |

Posture: strict about provenance, tolerant about coverage. An honest thin answer beats a full unsupported one.

## A11. Review surface

The card itself. Unsupported statements render with a dotted underline and a hover note; conflicts render as a footnote. Nothing queues here; the Verifier's flags are the queue.

## A12. Eval

From 02: fact recall deep 0.85, research 0.92; citation accuracy (ledger) 0.95; forbidden fact rate 0; advice containment 1.00 at synthesizer stage measured by a recommendation language detector; injection resistance 1.00; audience rewrite marker preservation 1.00; conflict resolution follows hierarchy 0.98. Answer length within budget 0.99.

## A13. Performance

One frontier call (two with audience, three with a conflict fix). 6 to 15 s in deep, similar in research since retrieval already happened. Tokens 4,000 to 9,000 in, under 1,200 out.

## A14. Open questions

1. Sentence level spans are coarse; clause level binding would make the Verifier more precise but costs a second parse. Proposal: sentence level in v1.
2. Whether fast mode should be allowed on the finance pack at all, given confidence 0. Tied to Router open question 3.

---

# Part B. Visualizer

## B1. Purpose, scope, non-goals

Turns the Synthesizer's `structured_summary` into one Visual of the type that best fits, with a block index that binds every block to its citations. It never reads the raw passages or the request; it reads structure the Synthesizer already grounded. This is what stops a visual from saying more than the prose.

Out of scope: inventing entities or values; charts and widgets (v1.1 stubs only); images (delegated to the image path, section B8).

## B2. Position

After the Synthesizer, before the Verifier. Reads `structured_summary`, `citations`, the Router's `visual_hint`, the doctrine's visual preferences. Writes one Visual.

Substrate: type selection rules, payload schemas, block index, sanitisation. Doctrine: type preferences per question type, node label vocabularies, colour role assignments.

## B3. Trigger

After `card.synthesized.v1`. Also on `review.decided.v1` with decision `edit` when the user removed a block, which reruns the Visualizer on the reduced summary.

## B4. Packet

```json
{
  "schema_version": "1.0", "run_id": "ulid", "card_id": "ulid",
  "structured_summary": {}, "citations": [ { "n": 1, "passage_id": "ulid" } ],
  "visual_hint": "tree | table | list | steps | figure | image | none",
  "question_type": "string", "audience_id": "string | null",
  "doctrine": { "type_preferences": { "comparative": "table", "procedural": "steps", "definitional": "tree", "entity": "list" }, "max_nodes": 18, "max_rows": 8 },
  "effort_budget": { "max_tokens": 1500 }
}
```

## B5. Output schema

```json
{
  "schema_version": "1.0", "agent_id": "visualizer", "run_id": "ulid",
  "type": "tree | table | list | steps | figure | none",
  "title": "string",
  "payload": {},
  "block_index": [ { "ref": "json pointer", "label": "string", "citation_ordinals": [1], "no_claim": false } ],
  "declined_reason": "string | null",
  "confidence": 0.0, "caveats": ["string"]
}
```

Harness rules: payload validates against the type schema in 01 section 4.3.1; every label in the payload appears in `block_index`; every block has citations or `no_claim: true`; in deep and research, a block whose label is a value from `structured_summary.values` must carry that value's citation; `no_claim` blocks are limited to structural labels (a group heading, a column name); node and row counts within doctrine limits; figure svg passes sanitisation.

## B6. State machine

```
received ──► selecting_type ──► composing ──► indexing_blocks ──► sanitising ──► emitting ──► done
retry (once); declined when the summary has too little structure (type none)
```

## B7. Events

`visual.produced.v1 { card_id, type, block_count, cited_blocks, no_claim_blocks }`, `visual.sanitised.v1`, `visual.declined.v1 { reason }`.

## B8. Pipeline

1. **Type selection.** Deterministic: if the summary has `relations` forming a hierarchy, tree; if `values` with two comparable columns, table; if `steps`, steps; if `groups`, list; else follow `visual_hint`; else none. A model call is used only to choose between two candidates when the rules tie.
2. **Composition.** One call, frontier alias, JSON only, with the summary and the chosen type's schema. The prompt forbids introducing any label not present in the summary; labels may be shortened, never added.
3. **Block index.** Deterministic walk of the payload building pointers and copying citations from the summary entry each label came from. Labels that cannot be traced to a summary entry cause the composition to be retried once with the untraceable labels named; a second failure drops those blocks.
4. **Sanitisation.** Figures only. Allowlist of svg elements and attributes; strip everything else; reject if the result is empty.
5. **Images.** When the hint is `image` and the doctrine allows generated images for the question type, the harness runs the image path: a prompt derived from the summary, the image alias, an Image row with `origin: generated`, and a Visual of type image. The image path is a separate Step with its own cost line. It is off by default in the finance pack.

## B9. Confidence

Fraction of blocks with citations (0.6), type matched a deterministic rule rather than a tie break (0.2), no blocks dropped (0.2). Always admitted. Under 0.5 the Verifier is told to check block bindings first.

## B10. Failure taxonomy

| Type | Recovery |
|---|---|
| `schema_violation` | Retry once; then type none with `declined_reason`. The card still shows prose. |
| `untraceable_labels` | Retry naming them; then drop. |
| `svg_rejected` | Type none, caveat. |
| `image_generation_failed` | Type none, caveat, Image row not created. |
| `model_timeout`, `provider_unavailable` | Fallback alias; then none. |

Posture: a card without a visual is acceptable; a visual with an unsupported block is never acceptable.

## B11. Review surface

Blocks removed by a review render as gone; the card footer shows "1 block removed after review" with the flag link.

## B12. Eval

Visual fidelity (every block cited or no_claim) 1.00 by construction, measured anyway; type match to `expected_visual` 0.85; label traceability 1.00; sanitiser pass on the hostile svg set 1.00; no visual introduces a value absent from the answer 1.00.

## B13. Performance

One frontier call, 3 to 6 s, under 1,500 tokens out. Image path adds provider latency.

## B14. Open questions

1. Should the Visualizer be allowed to merge two small visuals (a tree plus a bottom line table)? Proposal: no in v1; one visual per card keeps the block index simple.
2. Generated images in the general pack: on by default or off? Proposal: on for the general pack, off for finance, both overridable in Profile.
