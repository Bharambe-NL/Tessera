# 03. Router Agent v0.1

Product name: Tessera (confirmed by the owner 2026-08-30; the working name was Canvas). Register: working. Depends on: 01 Data Model, 02 Synthetic Data Generator. Load bearing patterns: 1 (state machine), 2 (task packet), 3 (events), 4 (failure taxonomy), 7 (output schema), 8 (policy engine), 13 (provenance), 21 (provider abstraction).

## 1. Purpose, scope, non-goals

The Router is the first agent to see a card request. It decides how much work the request deserves and which policy applies, then hands a resolved plan of record to the harness. It exists so that every later agent starts from a typed decision rather than from raw user text, and so that cost and depth are chosen once, in one place, with the user's override honoured.

In scope:

- Classify the request (question type, domain, audience, sensitivity).
- Choose the depth (fast, deep, research), respecting the user's override.
- Resolve the model policy for this run into concrete aliases per stage.
- Decide whether the Planner runs or the request goes straight to synthesis.
- Raise early flags for advice bait and out of scope requests, before any retrieval spends money.
- Detect that a card's context (parent answer, board seed, highlighted phrase) is stale and note it for the Planner.

Out of scope:

- Retrieval, answering, or producing a visual. The Router never calls a search tool and never writes prose the user sees.
- Overriding the user. If the user chose research, the Router may recommend fast in its output and must still route to research.
- Anything doctrine specific beyond reading the pack. The Router reads flag rules and audiences from the DoctrinePack; it does not contain them.

## 2. Architectural position

```
user / harness ──► Router ──► Planner ──► Retrievers ──► Synthesizer ──► Visualizer ──► Verifier ──► card
                      │
                      └──(fast path)──────────────────► Synthesizer ──► Visualizer ──► Verifier ──► card
```

The Router runs once per Run. It reads: the request, the Board, the parent Card if any, the Profile, the DoctrinePack, and a small number of recent Events on the board (last ten card requests) for pace and repetition detection. It writes one output row and emits events. It holds no state between runs.

Substrate parts: the state machine, packet, output schema, and policy resolution. Doctrine parts: the classification labels for domain and audience, the sensitivity rules, and the depth heuristics per domain, all read from the pack.

## 3. Trigger model

On demand only. The harness invokes the Router when any of these events occur:

- `card.requested.v1` from the composer (root), a card footer (follow), a highlight or block (branch), or a bundle fork continuation.
- `card.rerun.v1` with a changed depth or model override. The Router re-resolves policy; classification is reused from the previous run unless the question text changed.
- `read.completed.v1` when the Reader produced a card and the user asked a follow-up on it; the Router treats the Reader's output as the parent context.

The Router does not run on a schedule and does not run for Exercise generation; the Exercise agent has its own entry.

## 4. Typed task packet

```json
{
  "schema_version": "1.0",
  "run_id": "ulid",
  "card_id": "ulid",
  "request": {
    "text": "string",
    "kind": "root | follow | branch | read_follow",
    "anchor_text": "string | null",
    "anchor_block_ref": "string | null",
    "depth_override": "fast | deep | research | null",
    "model_override": { "stage": "string", "alias": "string" } | null,
    "audience_override": "string | null"
  },
  "board": {
    "board_id": "ulid",
    "title": "string",
    "default_depth": "fast | deep | research",
    "seed_label": "string | null",
    "context": "string | null",
    "doctrine_pack": { "code": "string", "version": "string" }
  },
  "parent": {
    "card_id": "ulid",
    "question": "string",
    "answer": "string",
    "depth": "string",
    "confidence": 0.0,
    "answered_at": "ISO8601",
    "citation_count": 0,
    "stale_citations": 0
  } | null,
  "profile": {
    "role": "string | null",
    "default_depth": "string",
    "model_policy": { "...": "data model section 5" }
  },
  "doctrine": {
    "audiences": [ { "id": "string", "name": "string" } ],
    "domains": [ "string" ],
    "sensitivity_rules": [ { "rule_id": "string", "detector": "string" } ],
    "depth_hints": { "domain": "fast | deep | research" }
  },
  "recent": [ { "question": "string", "depth": "string", "at": "ISO8601" } ],
  "effort_budget": { "max_tokens": 1500, "max_latency_ms": 2500 }
}
```

The packet is built by the harness. Amendment A1 applies: the effort budget is a field, the Router does not pick its own budget. The `recent` list is capped at ten and carries no answers, only questions, so the packet stays small.

## 5. Output schema

```json
{
  "schema_version": "1.0",
  "agent_id": "router",
  "run_id": "ulid",
  "classification": {
    "question_type": "factual | comparative | procedural | quantitative | regulatory | definitional | exploratory | meta",
    "domain": "string from doctrine.domains | unknown",
    "audience_id": "string | null",
    "language": "ISO 639-1",
    "needs_current_information": true,
    "needs_internal_documents": true,
    "needs_structured_data": false,
    "entities": [ "string" ],
    "is_follow_up_of_context": true
  },
  "depth": {
    "chosen": "fast | deep | research",
    "recommended": "fast | deep | research",
    "reason": "string, one sentence",
    "overridden_by_user": false
  },
  "plan_required": true,
  "visual_hint": "tree | table | list | steps | figure | image | none",
  "model_resolution": {
    "route": "alias", "plan": "alias", "synthesize": "alias",
    "visualize": "alias", "read": "alias", "verify": "alias"
  },
  "early_flags": [
    { "rule_id": "string", "severity": "info | warn | block", "reason": "string", "evidence": {} }
  ],
  "context_notes": {
    "parent_is_stale": false,
    "parent_stale_reason": "string | null",
    "repetition_of_recent": "ulid | null"
  },
  "confidence": 0.0,
  "caveats": [ "string" ]
}
```

Rules the harness enforces on receipt (Pattern 7):

- `depth.chosen` equals `request.depth_override` when the override is set. Any other value is a schema violation, retried once with the violation attached, then failed.
- `model_resolution` aliases must exist in `profile.model_policy.aliases`. A model override for one stage replaces that stage only.
- `early_flags[].rule_id` must exist in the doctrine pack.
- `classification.domain` must be a pack domain or `unknown`.

## 6. State machine and lifecycle

```
received ──► validating_packet ──► classifying ──► resolving_depth ──► resolving_policy
    ──► screening ──► emitting ──► done

any state ──► retry (once, on schema violation or timeout) ──► the same state
any state ──► failed (with failure type)
```

- `validating_packet`: harness side, deterministic. Missing doctrine or profile is a hard failure.
- `classifying`: one model call with the small alias. Output is the `classification` block.
- `resolving_depth`: deterministic with a model assisted tie break. Section 9.
- `resolving_policy`: deterministic. Merges profile policy, board override, card override.
- `screening`: runs the doctrine's sensitivity detectors. Deterministic detectors run in the harness; model backed detectors run as one batched call with the small alias.
- `emitting`: writes the Step, emits events, hands the output to the harness.

Target latency for the whole machine is under 2.5 seconds so the card's first state change is felt as immediate.

## 7. Events emitted

```
card.routed.v1 {
  card_id, run_id, question_type, domain, audience_id,
  depth_chosen, depth_recommended, overridden_by_user,
  plan_required, visual_hint, model_resolution
}
flag.raised.v1            { card_id, rule_id, severity, stage: "router" }        one per early flag
context.stale_noted.v1    { card_id, parent_card_id, reason }
model.call.v1             { stage: "route", alias, provider, tokens, latency_ms }
schema.violation.v1       { agent_id: "router", violations }
card.failed.v1            { failure }
```

The UI derives the "Routing…" then "Planning…" or "Answering…" states from `card.routed.v1`. The Router never writes UI text.

## 8. Reasoning pipeline

### 8.1 Classification

One prompt, small alias, JSON only. Inputs: request text, kind, anchor, parent question and the first 600 characters of the parent answer, board seed, audience list, domain list, profile role. The prompt asks for the `classification` block and nothing else. Entities are extracted as strings for the Planner; the Router does not resolve them against the Concept graph.

Two deterministic pre passes run before the model call and their results are inserted into the prompt as hints:

- Language detection from the request text.
- Keyword match against the doctrine's domain vocabulary. A strong single match sets `domain` without asking the model; the model call still runs for the rest of the block.

### 8.2 Depth resolution

In order:

1. If `request.depth_override` is set, `chosen` is that value. The Router still computes `recommended` and the reason, so the UI can show "you chose research; fast would probably do" without changing the run.
2. Otherwise start from `board.default_depth`.
3. Apply doctrine `depth_hints` for the classified domain. Regulatory and quantitative questions in the finance pack hint deep at minimum.
4. Apply request signals: `needs_current_information` or `needs_internal_documents` raises fast to deep. `question_type: comparative` with three or more entities, or `exploratory` with a broad scope, raises deep to research.
5. Apply context signals: a follow-up whose parent already has supported citations and whose question stays within the parent's entities may stay at the parent's depth or drop one level; the Router sets `is_follow_up_of_context` accordingly.
6. A branch spawned from a highlighted phrase inherits the parent's depth unless step 4 raises it.

Ties are broken toward the cheaper depth. The reason string names the step that decided.

### 8.3 Policy resolution

Deterministic merge, most specific wins: profile policy, then board `default_model_policy_id`, then `request.model_override`. Aliases that name a provider with no active key resolve to their fallback list; if no fallback has a key, the Router fails with `policy_unresolvable` rather than guessing. The resolved map is snapshotted onto the Run.

### 8.4 Screening

Runs the pack's sensitivity rules against the request text and, for follows, the parent answer. In the finance pack these are:

- `advice_request`: "should we", "recommend", "is it safe to", "what would you do". Deterministic regex list plus a model check for phrasing the list misses. Severity `warn`. The card still runs; the Synthesizer receives the flag and must answer descriptively; the Verifier checks that it did.
- `personal_data_in_request`: names with account numbers, national identifiers. Deterministic. Severity `block` on the request itself: the harness asks the user to remove the data before the run continues.
- `out_of_scope_domain`: domain `unknown` with no board context. Severity `info`. The card runs on the general pack rules.

Screening never blocks on a model backed detector alone; a model detector can raise `warn` at most.

### 8.5 Context notes

Deterministic. `parent_is_stale` is true when the parent has `stale_citations > 0` or when `answered_at` is older than the doctrine's freshness class for the parent's domain. `repetition_of_recent` is set when the same question text appears in `recent` within the last hour; the UI offers to open the existing card instead of paying for a rerun.

## 9. Confidence and auto-admit logic

Confidence is computed from deterministic signals, never self reported by the model (the Coffret rule):

| Signal | Contribution |
|---|---|
| Domain set by keyword match as well as by the model | +0.25 |
| Question type agreed by two independent prompts (the classifier and the screening call both return it) | +0.25 |
| Depth decided at step 1, 2, or 3 of 8.2 (explicit, default, or doctrine hint) | +0.25 |
| No early flags of severity warn or block | +0.15 |
| Language detected with high probability | +0.10 |

The Router's output is always admitted. There is no human review of routing in v1. Low confidence (under 0.5) does two things: the reason string is surfaced in the card header on hover, and the Planner receives `router_confidence` so it can widen its sub-question set. When confidence is under 0.3 and the user did not override, the Router raises the depth by one level as a hedge; the cost of a wrong fast route (an unverified answer to a regulatory question) is higher than the cost of a wrong deep route (a slower answer).

## 10. Failure taxonomy and recovery recipes

| Failure type | Cause | Recovery |
|---|---|---|
| `packet_invalid` | Missing profile, doctrine, or board. | Fail the run, emit `card.failed`, UI shows "The board's doctrine pack is missing" with a fix action. |
| `schema_violation` | Model output fails validation. | Retry once with violation detail. Then fall back to a deterministic default classification (domain from keywords or unknown, type factual, depth from board default) and set confidence 0.2. The run continues; the fallback is recorded. |
| `model_timeout` | Classification call over 2.5 s. | Same deterministic fallback. Emit `model.fallback.v1`. |
| `provider_unavailable` | The small alias's provider returns 5xx or auth error. | Try the fallback alias once. Then deterministic fallback. If the auth error persists across three runs, emit a profile level event so the UI shows the key as failing. |
| `policy_unresolvable` | No alias with an active key for a required stage. | Fail the run before any retrieval. UI opens Profile with the missing stage highlighted. |
| `override_conflict` | Depth override and a `block` early flag both present. | The flag wins. The run pauses at screening; `card.requested` is re-emitted when the user edits the request. |
| `unknown` | Anything else. | Pattern 14 evidence bundle: packet, partial outputs, model responses, timing. Fail the run. |

Recovery posture: tolerant. The Router should almost never stop a card; a wrong route is recoverable downstream and the user can rerun. The exceptions are missing keys and blocked requests, where continuing would waste money or send data the user did not intend.

## 11. Review queue surface

None in v1. The Router's decisions are visible, never queued. Two affordances on the card header:

- The depth badge shows `chosen`. Hovering shows `recommended` and the reason when they differ.
- "Rerun as…" opens the depth and model override menu, which re-emits `card.rerun.v1`.

Early flags of severity `block` appear inline in the composer before the run starts, as a request edit prompt rather than as a queue item.

## 12. Validation and eval methodology

Against the synthetic question set (02, section 6):

| Metric | Target |
|---|---|
| Route accuracy: `depth.recommended` equals `depth_expected` | 0.85 |
| Domain accuracy | 0.90 |
| Audience detection when an audience is implied by phrasing ("explain to engineering") | 0.90 |
| Advice bait detection at router stage | 0.95 recall, false positive rate under 0.05 on non bait questions |
| Personal data block | 1.00 recall on planted cases |
| Latency p95 | under 2.5 s |
| Cost per route | under 1,500 tokens |
| Override compliance | 1.00 (any miss is a schema bug) |

Deterministic mock testing (Pattern 18): a scenario file per failure type in section 10 with the mock provider returning malformed JSON, timeouts, and auth errors, asserting the recovery recipe ran and the events were emitted. Parity tests confirm the same packet produces the same classification across providers within a tolerance on the free text `reason` field only.

Regression: every change to the classification prompt or the depth rules reruns the 400 question set and diffs route accuracy by domain. A drop of more than 0.03 in any domain blocks the change.

## 13. Performance characteristics

- One model call in the common path, two when the model backed screening runs (batched). Small alias.
- Typical tokens: 800 to 1,200 in, under 300 out.
- Latency target 1.0 to 2.5 s. The UI shows "Routing…" only if the Router takes longer than 400 ms, to avoid a flicker on fast routes.
- Stateless; can be run in a worker process. Concurrency is bounded by the harness, not the agent.

## 14. Open questions

1. Should the Router be allowed to split one request into two cards when it detects two unrelated questions in one message? Proposal: no in v1. It routes the message as one card and adds a caveat; the Planner can still produce sub-questions. Splitting cards is a UX decision that belongs in the follow-up flow.
2. The repetition check uses exact text. A near duplicate check would help but needs an embedding call on every request. Proposal: exact match in v1, revisit when the local index exists anyway.
3. Whether `depth_hints` should be able to force a minimum depth that the user cannot lower (for instance, regulatory questions never fast). This is a doctrine decision with a product consequence. Proposal: the pack may set a minimum; the UI shows why fast is unavailable for this question.

## 15. Appendices

### A. Sample packet (branch from a highlight)

```json
{
  "schema_version": "1.0",
  "run_id": "01J...",
  "card_id": "01J...",
  "request": { "text": "Explain \"raw sensory data\" in this context", "kind": "branch",
               "anchor_text": "raw sensory data", "anchor_block_ref": null,
               "depth_override": null, "model_override": null, "audience_override": null },
  "board": { "board_id": "01J...", "title": "what are world models?", "default_depth": "fast",
             "seed_label": null, "context": null, "doctrine_pack": { "code": "general", "version": "1.0.0" } },
  "parent": { "card_id": "01J...", "question": "what are world models?", "answer": "World models are internal representations…",
              "depth": "fast", "confidence": 0.0, "answered_at": "2026-08-25T10:02:00+02:00",
              "citation_count": 0, "stale_citations": 0 },
  "profile": { "role": null, "default_depth": "fast", "model_policy": { "...": "..." } },
  "doctrine": { "audiences": [], "domains": ["general"], "sensitivity_rules": [], "depth_hints": {} },
  "recent": [ { "question": "what are world models?", "depth": "fast", "at": "2026-08-25T10:02:00+02:00" } ],
  "effort_budget": { "max_tokens": 1500, "max_latency_ms": 2500 }
}
```

### B. Sample output

```json
{
  "schema_version": "1.0", "agent_id": "router", "run_id": "01J...",
  "classification": { "question_type": "definitional", "domain": "general", "audience_id": null, "language": "en",
                      "needs_current_information": false, "needs_internal_documents": false, "needs_structured_data": false,
                      "entities": ["raw sensory data", "world model"], "is_follow_up_of_context": true },
  "depth": { "chosen": "fast", "recommended": "fast", "reason": "Definitional follow-up within parent scope; board default fast.", "overridden_by_user": false },
  "plan_required": false,
  "visual_hint": "steps",
  "model_resolution": { "route": "small", "plan": "medium", "synthesize": "frontier", "visualize": "frontier", "read": "vision", "verify": "medium" },
  "early_flags": [],
  "context_notes": { "parent_is_stale": false, "parent_stale_reason": null, "repetition_of_recent": null },
  "confidence": 0.75,
  "caveats": []
}
```

### C. Sample output, finance pack, advice bait with user override

Request: "Should we move the trading book exposures under the new CAR3 treatment before Q4?" with `depth_override: fast`.

```json
{
  "classification": { "question_type": "regulatory", "domain": "capital", "audience_id": null, "language": "en",
                      "needs_current_information": true, "needs_internal_documents": true, "needs_structured_data": true,
                      "entities": ["trading book", "CAR3", "Q4"], "is_follow_up_of_context": false },
  "depth": { "chosen": "fast", "recommended": "research", "reason": "Regulatory and quantitative with internal data need; doctrine hint deep, comparative scope raises to research.", "overridden_by_user": true },
  "plan_required": false,
  "visual_hint": "table",
  "early_flags": [ { "rule_id": "advice_request", "severity": "warn", "reason": "The question asks for a recommendation.", "evidence": { "matched": "Should we" } } ],
  "confidence": 0.6,
  "caveats": [ "Fast depth on a regulatory question yields an unverified card." ]
}
```

The run proceeds at fast because the user chose it. The card header shows the recommendation and the advice flag travels to the Synthesizer and Verifier.

Next document: 04, the Planner agent.
