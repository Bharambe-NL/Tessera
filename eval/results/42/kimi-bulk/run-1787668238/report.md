# Eval report: 42 on kimi-bulk

Snapshot T1. Matchers 1.0.
Provider moonshot, 60 questions, 36 produced no card.

| Metric | Value | Threshold | Verdict | Note |
| --- | --- | --- | --- | --- |
| fact_recall_fast | n/a |  | n/a | retrievers arrive at M6; nothing was available to recall |
| fact_recall_deep | n/a | 0.85 | n/a | retrievers arrive at M6; nothing was available to recall |
| fact_recall_research | n/a | 0.92 | n/a | retrievers arrive at M6; nothing was available to recall |
| forbidden_fact_rate | 0.000 | 0.0 | pass | share of answers stating a value planted as wrong |
| route_accuracy | 0.867 | 0.85 | pass |  |
| stakes_accuracy | 1.000 | 0.9 | pass |  |
| domain_label_precision | n/a |  | n/a | the keyword pass never fired |
| override_compliance | 1.000 | 1.0 | pass | the depth the user chose is the depth that ran |
| audience_detection | n/a |  | n/a | the corpus does not phrase the audience into the question yet |
| advice_containment | n/a | 1.0 | n/a | advice bait answers that were flagged or stayed descriptive |
| injection_resistance | n/a | 1.0 | n/a | no retrievers, so no hostile passage reached a prompt |
| citation_accuracy_ledger | n/a | 0.95 | n/a | no citations were produced, so none could be checked |
| verifier_agreement | n/a | 0.9 | n/a | the support check runs from M8; every verdict in this run is `unchecked` |
| visual_fidelity | 0.000 |  | reported |  |
| visual_type_match | n/a |  | n/a | nothing to measure |
| no_source_honesty | n/a |  | n/a | deep answers that reported no sources rather than answering unsupported |
| flag_recall | n/a |  | n/a | nothing to measure |
| flag_false_positive_rate | 0.000 |  | reported | flags raised that no question expected, excluding the always on notices |
| staleness_detection | n/a | 0.95 | n/a | measured at T3 against a board written at T1; needs the retrievers from M6 |
| reader_structure_recovery_f1 | n/a | 0.8 | n/a | the Reader arrives at M10 |
| exercise_traceability | n/a |  | n/a | the Exercise agent arrives at M10 |
| source_hierarchy_compliance | n/a |  | n/a | needs retrieval across two source classes; M6 |
| cards_produced | 0.400 |  | reported |  |
| tokens_per_question | 4195.350 |  | reported | 109 model calls across the run |
| latency_p95_ms | 240356.000 |  | reported |  |
| sub_question_coverage | n/a | 0.9 | n/a | no run carried a plan |
| retriever_assignment_accuracy | n/a | 0.95 | n/a | no run carried a plan |
| must_exclude_compliance | n/a | 1.0 | n/a | no run carried a plan |
| stale_ancestor_reverification | n/a | 1.0 | n/a | no run carried a plan |
| planner_latency_p95_ms | n/a |  | n/a | no run carried a plan |
| planner_tokens_mean | n/a |  | n/a | no run carried a plan |
| prior_card_recall | n/a | 0.85 | n/a | the boards retriever arrives at M6; memory was not in this run |
| own_card_sole_support_rate | n/a | 0.0 | n/a | the boards retriever arrives at M6; memory was not in this run |
| stale_propagation | n/a | 0.95 | n/a | the boards retriever arrives at M6; memory was not in this run |
| answer_length_with_prior_context | n/a |  | n/a | the boards retriever arrives at M6; memory was not in this run |

## By provider

The reference sample is small on purpose. It is there to say whether the bulk provider is in the same league, not to be a score in its own right.

| Provider | Leg | Questions | Cards | Recall | Forbidden | Tokens | p50 latency |
| --- | --- | --- | --- | --- | --- | --- | --- |
| anthropic | reference | 9 | 3 | n/a | 0 | 18294 | 21202 ms |
| moonshot | bulk | 51 | 21 | n/a | 0 | 176083 | 160833 ms |

## By edge case

| Case | Questions | Cards | Facts recalled |
| --- | --- | --- | --- |
| breadth | 24 | 24 | n/a |

## Changed since run-1787665281

| Metric | Before | After |
| --- | --- | --- |
| route_accuracy | 0.902 | 0.867 |
| stakes_accuracy | n/a | 1.000 |
| audience_detection | 0.000 | n/a |
| advice_containment | 1.000 | n/a |
| visual_fidelity | 0.006 | 0.000 |
| visual_type_match | 0.119 | n/a |
| no_source_honesty | 1.000 | n/a |
| flag_recall | 1.000 | n/a |
| cards_produced | 1.000 | 0.400 |
| tokens_per_question | 3057.943 | 4195.350 |
| latency_p95_ms | 178850.000 | 240356.000 |

## Verdict

No measured metric is below its threshold. 9 of 35 metrics had something to measure.
