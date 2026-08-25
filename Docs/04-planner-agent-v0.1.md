# 04. Planner Agent v0.1

Register: working. Depends on: 01, 02, 03. Load bearing patterns: 1, 2, 3, 4, 7; also 5 (context freshness gate) and 16 (team memory, read only).

## 1. Purpose, scope, non-goals

The Planner turns one routed request into a retrieval plan: a small set of sub-questions, each bound to the retrievers that should answer it, with the constraints the Synthesizer must honour. It runs only when the Router set `plan_required`. In fast mode it does not run.

In scope: sub-question decomposition; retriever assignment per sub-question; entity resolution against the Concept graph; carrying the board context (parent answer, seed, highlighted phrase) into each sub-question; declaring boundary constraints (must include, must exclude); estimating the retrieval budget.

Out of scope: retrieval itself; answering; deciding depth (already fixed by the Router); writing anything the user reads.

## 2. Architectural position

Between Router and Retrievers. Reads the Router output, the parent Card chain (up to three ancestors), confirmed Concepts linked to the board, and the doctrine pack's retriever list. Writes one plan. The harness fans the plan out to retrievers in parallel.

Substrate: decomposition mechanics, packet, schema, budget accounting. Doctrine: which retrievers exist, their default order, domain vocabulary for entity resolution, must exclude rules (for instance "never retrieve from the Sensitive folder").

## 3. Trigger model

On demand, after `card.routed.v1` with `plan_required: true`. Also on `card.rerun.v1` when depth changes from fast to deep or research. Not on schedule.

## 4. Typed task packet

```json
{
  "schema_version": "1.0",
  "run_id": "ulid", "card_id": "ulid",
  "request": { "text": "string", "kind": "root | follow | branch | read_follow", "anchor_text": "string | null", "anchor_block_ref": "string | null" },
  "routing": { "question_type": "string", "domain": "string", "audience_id": "string | null", "entities": ["string"],
               "needs_current_information": true, "needs_internal_documents": true, "needs_structured_data": false,
               "depth": "deep | research", "router_confidence": 0.0, "early_flags": [] },
  "context": {
    "board_seed": "string | null", "board_context": "string | null",
    "ancestors": [ { "card_id": "ulid", "question": "string", "answer_excerpt": "string (first 800 chars)", "citations": [ { "ordinal": 1, "source_title": "string", "source_class": "string", "stale": false } ] } ],
    "parent_visual_block": { "ref": "string", "label": "string", "note": "string" } | null
  },
  "concepts": [ { "concept_id": "ulid", "term": "string", "definition": "string", "aliases": ["string"] } ],
  "retrievers": [ { "id": "web | local | regulatory | structured", "enabled": true, "config_summary": "string" } ],
  "doctrine": { "must_exclude": ["string"], "domain_vocabulary": ["string"], "freshness_classes": {} },
  "effort_budget": { "max_tokens": 2500, "max_sub_questions": 3, "max_passages_total": 40 }
}
```

`max_sub_questions` is 3 for research and 1 for deep (deep is a single search pass with a plan that still declares retrievers and constraints). The harness sets it from depth.

## 5. Output schema

```json
{
  "schema_version": "1.0", "agent_id": "planner", "run_id": "ulid",
  "sub_questions": [
    {
      "sq_id": "string", "text": "string",
      "purpose": "string, one sentence",
      "retrievers": [ { "id": "string", "query": "string", "filters": { "corpus": "string | null", "folder": "string | null", "date_from": "ISO8601 | null", "version_ref": "string | null" }, "max_passages": 12 } ],
      "entity_refs": ["concept_id | literal"],
      "depends_on": ["sq_id"]
    }
  ],
  "constraints": {
    "must_include": ["string"],
    "must_exclude": ["string"],
    "answer_scope": "string, one sentence describing what the answer covers",
    "audience_id": "string | null",
    "value_policy": "cite_only | cite_or_query",
    "stale_ancestor_citations": [ { "card_id": "ulid", "ordinal": 1 } ]
  },
  "resolved_entities": [ { "literal": "string", "concept_id": "ulid | null", "ambiguity": "none | multiple | unknown" } ],
  "budget": { "passages_total": 0, "estimated_tokens": 0 },
  "confidence": 0.0, "caveats": ["string"]
}
```

Harness rules: every `retrievers[].id` must be enabled; `sub_questions` length within budget; `depends_on` acyclic; `must_exclude` contains every doctrine must exclude entry (the Planner may add, never remove); `value_policy` is `cite_only` unless the structured retriever is assigned.

## 6. State machine

```
received ──► validating ──► resolving_entities ──► decomposing ──► assigning_retrievers
   ──► constraining ──► budgeting ──► emitting ──► done
retry (once) on schema violation; failed on packet_invalid or budget_impossible
```

## 7. Events

```
card.planned.v1  { card_id, run_id, sub_question_count, retriever_ids, passages_budget, audience_id }
entity.resolved.v1 { card_id, literal, concept_id, ambiguity }
model.call.v1, schema.violation.v1, card.failed.v1
```

## 8. Reasoning pipeline

1. **Entity resolution.** Deterministic first: literal match of Router entities against Concept terms and aliases in the pack. Ambiguous literals (two concepts) are marked `multiple` and both definitions go into the decomposition prompt, which must pick or ask. Unknown literals stay literal.
2. **Context freshness gate (Pattern 5).** For each ancestor citation flagged stale, the Planner adds a sub-question that re-verifies that value, and lists the citation under `stale_ancestor_citations` so the Synthesizer knows the parent's number may be wrong. A stale ancestor never silently becomes context.
3. **Decomposition.** One model call (medium alias). Inputs: request, routing, ancestors, resolved entities, retriever list. Output: sub-questions with purposes. Deep depth yields exactly one sub-question that restates the request precisely. Research yields two or three that partition the request (definition, current rule, internal position, comparison) without overlap.
4. **Retriever assignment.** Deterministic rules with model assistance for the query text. Regulatory questions always include the regulatory retriever. `needs_internal_documents` adds local. `needs_current_information` adds web. `needs_structured_data` adds structured and sets `value_policy: cite_or_query`. Each assignment gets a query written for that retriever (keyword style for local and web, article style for regulatory, a query template for structured).
5. **Constraints.** `must_include` carries the anchor text or block label and any value the request names. `must_exclude` merges doctrine exclusions with request scope limits ("only the trading book"). `answer_scope` is one sentence the Verifier will check the answer against.
6. **Budgeting.** Sum of `max_passages`, capped by `max_passages_total`. If the cap cannot be met with at least four passages per sub-question, drop the lowest priority sub-question and add a caveat.

## 9. Confidence and auto-admit

Deterministic signals: all entities resolved without ambiguity (+0.3), every sub-question has at least two retrievers or one regulatory retriever (+0.2), no stale ancestors (+0.2), decomposition passed a self consistency check (a second cheap call confirms the sub-questions cover the request) (+0.3). Always admitted. Below 0.5 the Synthesizer is told to state scope limits in its answer.

## 10. Failure taxonomy

| Type | Recovery |
|---|---|
| `schema_violation` | Retry once; then fall back to a single sub-question equal to the request with all enabled retrievers. |
| `no_retriever_enabled` | Fail with a Profile prompt to enable at least web or local. |
| `budget_impossible` | Reduce to one sub-question; caveat. |
| `entity_ambiguous_unresolved` | Proceed with both readings; the Synthesizer must present both; caveat. |
| `model_timeout`, `provider_unavailable` | Fallback alias, then the single sub-question fallback. |
| `unknown` | Evidence bundle, fail. |

Posture: tolerant. A weak plan still produces a card; the Verifier catches what the plan missed.

## 11. Review surface

None. The plan is visible under the card's "How this was built" disclosure as the list of sub-questions and retrievers, rendered from `card.planned.v1`.

## 12. Eval

Against the research and deep subset of the question set: sub-question coverage (required facts reachable through at least one sub-question's retriever and filters) 0.90; retriever assignment accuracy against `required_sources` classes 0.95; stale ancestor re-verification 1.00 on the T3 snapshot; must exclude compliance 1.00; latency p95 under 4 s; tokens under 2,500.

## 13. Performance

One or two medium calls. 3 to 4 s. Stateless.

## 14. Open questions

1. Should the Planner be allowed to ask the user a clarifying question when ambiguity is `multiple`? Proposal: no in v1; present both readings. A clarifying prompt is a UX flow to add once the flag review surface exists.
2. Sub-question parallelism is fixed at 3. Research on very broad questions may want 5. Revisit with cost data.

## 15. Appendix: sample plan (research, finance pack)

Request: "How does CAR3 change the treatment of trading book exposures for Meerkant Bank, and what does our current policy say?"

Sub-questions: (1) "What does CAR3 v2 say about trading book exposure treatment, and which articles changed from v1?" retrievers regulatory (version_ref CAR3-v2) and web; (2) "What is Meerkant Bank's current internal policy on trading book exposure treatment?" retriever local (folder Policies); (3) "What are the exposure figures the policy applies to?" retriever structured, `value_policy: cite_or_query`. Constraints: must include "trading book", must exclude "Sensitive", scope "the change in treatment and the internal position, without recommending an action".
