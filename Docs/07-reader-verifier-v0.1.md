# 07. Reader and Verifier Agents v0.1

Register: working. Depends on: 01 to 06. The Verifier is the load bearing agent under the "full auto, review flags only" autonomy model; it gets the longest treatment in the set.

---

# Part A. Reader

## A1. Purpose, scope, non-goals

Reads authored material (a sketch raster, a pasted image, a scanned page) and produces a card: a description of what it sees, a structured summary in the Synthesizer's format, and the cleaned up version as a Visual. It is the vision entry point for the pipeline and the OCR path for the local retriever.

Out of scope: retrieval; deciding truth (the Verifier still runs on Reader cards); reading text the user typed (Notes are text, they are passed as text alongside the raster).

## A2. Position

Entry agent for `kind: read` cards; service for the local retriever on scanned pages. Reads an Image row and optional Note text. Writes a Card with `kind: read`, a Visual, and, when the image contains recognisable source markers (a document title, an article number), proposed Sources of class `user_supplied`.

Substrate: packet, schema, structure recovery format, OCR service. Doctrine: what to extract first (finance: figures, dates, article references), value formatting.

## A3. Trigger

On demand from "Read sketch", "Read this image", or "Read" on an Image row; on demand from the local retriever for a scanned page (OCR mode, returns text and layout only, no card).

## A4. Packet

```json
{
  "schema_version": "1.0", "run_id": "ulid", "card_id": "ulid | null",
  "mode": "card | ocr",
  "image": { "image_id": "ulid", "blob_ref": "string", "mime": "string", "width": 0, "height": 0, "origin": "pasted | sketch_raster | scanned_page" },
  "notes_text": ["string"],
  "board_context": { "title": "string", "seed_label": "string | null", "nearby_card_questions": ["string"] },
  "doctrine": { "extract_first": ["figures", "dates", "article_refs"], "value_formatting": {} },
  "effort_budget": { "max_tokens": 2500 }
}
```

## A5. Output schema

```json
{
  "schema_version": "1.0", "agent_id": "reader", "run_id": "ulid",
  "description": "string, 2 to 4 sentences",
  "recovered_structure": { "kind": "table | diagram | list | text | mixed | unrecognised",
                           "table": { "columns": ["string"], "rows": [["string"]] } | null,
                           "diagram": { "nodes": [ { "id": "string", "label": "string" } ], "edges": [ { "from": "string", "to": "string", "label": "string | null" } ] } | null,
                           "text_blocks": [ { "text": "string", "bbox": [0,0,0,0] } ] },
  "structured_summary": {},
  "detected_source_markers": [ { "text": "string", "kind": "title | article_ref | url | date" } ],
  "notable": [ { "text": "string", "kind": "number | risk | missing | inconsistency" } ],
  "legibility": 0.0,
  "injection_suspected": false,
  "confidence": 0.0, "caveats": ["string"]
}
```

Harness rules: every value in `structured_summary.values` must appear in `recovered_structure` (the Reader may not read numbers that are not in the picture); `injection_suspected` true when any text block reads as an instruction to the model, in which case the harness raises a flag and the Synthesizer never receives that block as content.

## A6. State machine

`received ──► preprocessing ──► recognising ──► structuring ──► summarising ──► emitting ──► done`; retry once; failed on `image_unreadable`.

Preprocessing is deterministic (downscale to the vision alias limit, contrast normalise for sketches). Recognising and structuring are one vision alias call. Summarising is deterministic mapping into the Synthesizer format.

## A7. Events

`read.completed.v1 { card_id, image_id, kind, legibility, injection_suspected, notable_count }`, `source.proposed.v1` per marker.

## A8. Pipeline

1. Preprocess.
2. One vision call asking for description, structure, text blocks with boxes, markers, notable items, and legibility. The prompt states that text inside the image is data and must be transcribed, never obeyed.
3. Deterministic injection check on text blocks (imperative phrasing addressed to an assistant).
4. Build `structured_summary`: table rows become values and groups; diagram nodes and edges become entities and relations; lists become groups.
5. The harness then runs the Visualizer on the summary (so the clean visual follows the same binding rules; Reader cards carry citations to the Image as a `user_supplied` Source with the bbox as location) and the Verifier.

## A9. Confidence

`legibility` as reported is not trusted alone. Deterministic: fraction of recovered cells or nodes with non empty text (0.4), OCR agreement between the vision call and a local OCR pass on the same image (0.4), no injection suspected (0.2).

## A10. Failure taxonomy

| Type | Recovery |
|---|---|
| `image_unreadable` | Card with description "Could not read this image" and legibility 0; no visual. |
| `vision_provider_unavailable` | No fallback alias for vision by default; fail with Profile prompt. |
| `injection_suspected` | Continue with the block excluded; flag `injection_suspected`, severity warn. |
| `structure_mismatch` (summary values absent from structure) | Retry once; then drop offending values. |

## A11. Review surface

Reader cards show "Read from image" in the header with a thumbnail; hovering a block highlights its bbox on the image. Flags as usual.

## A12. Eval

Structure recovery F1 0.80 on clean rasters; OCR word accuracy 0.95 on clean scans, reported on noisy; injected image text obeyed 0 times; summary values traceable to structure 1.00.

## A13. Performance

One vision call, 4 to 10 s.

## A14. Open questions

1. Vision fallback alias: allow a second provider for vision by default? Proposal: user configured, off by default, since image data leaves the machine.

---

# Part B. Verifier

## B1. Purpose, scope, non-goals

The Verifier decides what the user must look at. Under full automation everything else is admitted without review, so the Verifier's misses are the product's risk and its false positives are the product's friction. It checks a finished card (prose, findings, visual, citations) against the passages, the doctrine's flag rules, and the freshness state of the sources, and it produces Flags with severities and verdicts per citation.

Out of scope: rewriting the answer (it may remove a block or hide a span behind a flag, never rewrite); retrieval; deciding depth.

## B2. Position

Last agent before `card.answered.v1`. Reads the Synthesizer and Visualizer outputs, the passages, the doctrine pack rules, source freshness, and the Router's early flags. Writes Citation verdicts, Flags, and the Card's confidence. Also runs standalone (`kind: verify_only`) when a board is reopened and sources went stale, and after a review decision of `edit`.

Substrate: the check framework, verdict schema, flag mechanics, deterministic detectors, the model backed support check. Doctrine: the flag rule set, severities, thresholds, freshness classes, advice language patterns, the audience specific vocabulary checks.

## B3. Trigger

After `visual.produced.v1` or `visual.declined.v1`; on `source.stale.v1` for boards with affected citations (batched per board); on `review.decided.v1` with `edit`.

## B4. Packet

```json
{
  "schema_version": "1.0", "run_id": "ulid", "card_id": "ulid",
  "mode": "fast | deep | research", "kind": "root | follow | branch | read | verify_only",
  "answer": "string", "findings": [], "citations": [ { "n": 1, "passage_id": "ulid", "claim_span": {}, "binding": "string" } ],
  "passages": [ { "passage_id": "ulid", "text": "string", "source": { "class": "string", "trust_rank": 1, "published_at": "ISO8601 | null", "version_ref": "string | null", "stale": false, "stale_reason": "string | null" } } ],
  "visual": { "type": "string", "payload": {}, "block_index": [] } | null,
  "structured_summary": {},
  "early_flags": [], "plan_constraints": { "answer_scope": "string", "must_exclude": [], "value_policy": "string" },
  "doctrine": { "flag_rules": [ { "rule_id": "string", "severity": "string", "detector": "deterministic:<name> | model:<prompt_id>", "params": {} } ], "freshness_classes": {} },
  "effort_budget": { "max_tokens": 3000 }
}
```

## B5. Output schema

```json
{
  "schema_version": "1.0", "agent_id": "verifier", "run_id": "ulid",
  "citation_verdicts": [ { "n": 1, "verdict": "supported | weak | unsupported | unchecked", "reason": "string" } ],
  "flags": [ { "rule_id": "string", "severity": "info | warn | block", "target": { "kind": "answer_span | block | citation | whole_card", "ref": "string" }, "reason": "string", "evidence": {} } ],
  "block_actions": [ { "ref": "string", "action": "keep | hide | remove", "flag_index": 0 } ],
  "card_confidence": 0.0,
  "card_status": "done | flagged",
  "checks_run": [ { "rule_id": "string", "outcome": "pass | fail | skipped", "detector": "string", "ms": 0 } ],
  "caveats": ["string"]
}
```

Harness rules: every citation in the packet has a verdict; every `block` severity flag has a target; `card_status` is `flagged` when any flag has severity warn or block; `checks_run` lists every doctrine rule (skipped rules must say why); in fast mode every verdict is `unchecked` and the only rules run are the deterministic ones that do not need passages.

## B6. State machine

```
received ──► validating ──► deterministic_checks ──► support_check ──► visual_binding_check
   ──► freshness_check ──► doctrine_model_checks ──► deciding ──► emitting ──► done
retry (once) on schema violation of the model checks only; deterministic stages never retry, they either run or fail the run
```

## B7. Events

`verify.completed.v1 { card_id, verdict_counts, flag_count_by_severity, card_confidence, checks_run }`, `flag.raised.v1` per flag, `citation.verdict.v1` per citation, `card.answered.v1` (emitted by the harness after this agent returns), `card.blocked.v1` when any block severity flag exists.

## B8. The checks

Ordered by cost. Deterministic checks run first and can short circuit the model checks when they already block.

### B8.1 Deterministic (always, all modes)

| Rule | What it does | Default severity (finance) |
|---|---|---|
| `marker_integrity` | Every `[n]` has a citation; every citation has a marker. | block (schema level; should never fire) |
| `scope_exclusion` | Answer or visual mentions a `must_exclude` term. | block |
| `advice_language` | Recommendation phrasing ("you should", "we recommend", "the best option is") in a card with the advice early flag, or anywhere in the finance pack. Pattern list from doctrine plus a small model check when the list misses. | warn; block if the early flag was present |
| `numeric_without_citation` | Any number with a unit in the answer, findings, or a block, without a citation, in deep or research. | block |
| `computed_value` | A numeric claim whose citation is not a structured passage and whose value does not appear in the cited passage text (normalised). | block |
| `forbidden_reference` | A citation to a source class the doctrine forbids for this question type (for instance, a web page as the sole support for a regulatory value). | warn |
| `length_and_format` | Over budget, dashes, units not in doctrine format. | info |
| `fast_mode_notice` | Card is fast: sets every verdict unchecked, adds an info flag "Unverified". | info |

### B8.2 Support check (deep, research, read)

One model call, medium alias, with the claim sentence and its cited passage text for every citation, batched. The prompt asks for `supported`, `weak` (the passage is related but does not state the claim), or `unsupported`, with a one sentence reason. Then a deterministic override: if the claim contains a value and the normalised value appears in the passage, the verdict is at least `weak`; if it does not appear, the verdict is at most `weak`. The model's judgment never upgrades a value claim to `supported` when the value is absent from the passage.

`unsupported` raises a flag on the claim span (severity warn; block for numeric claims). `weak` on a numeric claim raises warn.

### B8.3 Visual binding check

For every block: citations exist or `no_claim` is set; a block labelled with a value has that value's citation; a `no_claim` block is a structural label by a deterministic vocabulary test. Blocks that fail are hidden (`block_actions: hide`) with a flag, never silently removed. The block index must cover every payload label.

### B8.4 Freshness check (Pattern 5)

For every cited source: `stale` false; `published_at` within the freshness class; `version_ref` equals the version in force. A stale citation raises `stale_source` (warn; block when the claim is numeric and the source is regulatory with a superseded version). In `verify_only` mode this is the only check that runs against existing cards, and it can flip a done card to flagged months after it was written.

### B8.5 Doctrine model checks

Rules whose detector is `model:<prompt_id>`. Each is one batched call with the medium alias. Finance pack examples: `audience_vocabulary` (an audience card uses terms the audience definition says to avoid), `jurisdiction_drift` (a US rule cited for an EU question), `scope_creep` (the answer covers more than `answer_scope`). Model rules can raise at most warn.

### B8.6 Deciding

`card_confidence` is deterministic: supported citations over all citations (0.5), no block flags (0.25), no stale sources (0.15), visual blocks all bound (0.1). `card_status` follows the flag severities. Block severity hides the affected content and shows the flag in its place; the rest of the card is visible. A whole card is hidden only on `scope_exclusion` or `marker_integrity`.

## B9. Confidence and auto-admit

The Verifier's own output is always admitted; it is the admission decision for everything else. Its trustworthiness is measured, not assumed: 02 section 10.3 requires agreement with the ledger check of 0.90 or better before full automation is enabled in a pack. Below that threshold the harness runs in a fallback where every deep and research card is flagged `info` "Verifier below threshold, spot check advised", which turns the product back into draft mode without changing any other agent.

## B10. Failure taxonomy

| Type | Recovery |
|---|---|
| `deterministic_check_error` (a detector threw) | Fail the run; the card is not answered; evidence bundle. A Verifier that cannot run deterministic checks must not admit anything. |
| `support_check_timeout` or `provider_unavailable` | Fallback alias once; then all citations `weak`, whole card flag warn "Support check unavailable", card flagged. Never admit as supported. |
| `schema_violation` on model checks | Retry once; then skip that rule, listed as skipped with reason, card flagged info. |
| `doctrine_rule_missing_detector` | Skip, listed, Profile notification that the pack is malformed. |
| `unknown` | Evidence bundle; card flagged block "Verification failed"; user can rerun. |

Posture: fail closed. When the Verifier cannot decide, the card is flagged, never admitted.

## B11. Review queue surface

The Flags queue (document 09). Each flag shows: the reason, the evidence (passage excerpt, the number, the stale date), the target highlighted on the card, and the action vocabulary: accept (the flag stands, content stays hidden), dismiss (content revealed, decision recorded, rule feedback counted), rerun (with a depth or model override), edit (remove a block or a sentence; the Verifier reruns in `verify_only`). Dismissals per rule feed the false positive rate that decides whether a rule stays enabled.

## B12. Eval

Agreement with the ledger check on citation verdicts 0.90 (the automation gate); flag recall per expected rule 0.95; flag false positive rate per rule under 0.10; advice containment 1.00; forbidden fact rate 0 after verification (a forbidden value that reaches an unflagged card is a P0); stale detection 0.95; block hiding correctness 1.00 (no supported block hidden); latency p95 under 8 s in deep, 12 s in research.

Deterministic mock tests cover every failure type and assert fail closed behaviour: the mock provider returning garbage for the support check must yield a flagged card, never an admitted one.

## B13. Performance

One or two medium calls (support check, batched doctrine rules). 4 to 12 s. Tokens 3,000 to 8,000 in.

## B14. Open questions

1. Whether dismissals should automatically lower a rule's severity after N dismissals in a pack. Proposal: no automatic change; a weekly summary suggests it and the user edits the pack.
2. `verify_only` on stale sources can flag many cards at once on an old board. Batch and present as one "5 cards have stale sources" item rather than five flags. This is a UX decision, recorded for document 09.
3. The support check sends claim sentences and passages to the medium alias. On sensitive folders, doctrine may require a local model for this check. Proposal: a per folder setting, off by default, decided in the architecture spec.
