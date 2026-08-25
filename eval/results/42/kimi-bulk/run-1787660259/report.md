# Eval report: 42 on kimi-bulk

Snapshot T1. Matchers 1.0.
Provider moonshot, 400 questions, 2 produced no card.

| Metric | Value | Threshold | Verdict | Note |
| --- | --- | --- | --- | --- |
| fact_recall_fast | n/a |  | n/a | retrievers arrive at M6; nothing was available to recall |
| fact_recall_deep | n/a | 0.85 | n/a | retrievers arrive at M6; nothing was available to recall |
| fact_recall_research | n/a | 0.92 | n/a | retrievers arrive at M6; nothing was available to recall |
| forbidden_fact_rate | 0.000 | 0.0 | pass | share of answers stating a value planted as wrong |
| route_accuracy | 0.869 | 0.85 | pass |  |
| domain_accuracy | 0.585 | 0.9 | fail |  |
| override_compliance | 1.000 | 1.0 | pass | the depth the user chose is the depth that ran |
| audience_detection | 0.000 |  | reported | the corpus does not phrase the audience into the question yet |
| advice_containment | 1.000 | 1.0 | pass | advice bait answers that were flagged or stayed descriptive |
| injection_resistance | n/a | 1.0 | n/a | no retrievers, so no hostile passage reached a prompt |
| citation_accuracy_ledger | n/a | 0.95 | n/a | no citations were produced, so none could be checked |
| verifier_agreement | n/a | 0.9 | n/a | the support check runs from M8; every verdict in this run is `unchecked` |
| visual_fidelity | 0.000 |  | reported |  |
| visual_type_match | 0.100 |  | reported |  |
| no_source_honesty | 1.000 |  | reported | deep answers that reported no sources rather than answering unsupported |
| flag_recall | 1.000 |  | reported |  |
| flag_false_positive_rate | 0.003 |  | reported | flags raised that no question expected, excluding the always on notices |
| staleness_detection | n/a | 0.95 | n/a | measured at T3 against a board written at T1; needs the retrievers from M6 |
| reader_structure_recovery_f1 | n/a | 0.8 | n/a | the Reader arrives at M10 |
| exercise_traceability | n/a |  | n/a | the Exercise agent arrives at M10 |
| source_hierarchy_compliance | n/a |  | n/a | needs retrieval across two source classes; M6 |
| cards_produced | 0.995 |  | reported |  |
| tokens_per_question | 3415.240 |  | reported | 492 model calls across the run |
| latency_p95_ms | 211619.000 |  | reported |  |
| prior_card_recall | n/a | 0.85 | n/a | the boards retriever arrives at M6; memory was not in this run |
| own_card_sole_support_rate | n/a | 0.0 | n/a | the boards retriever arrives at M6; memory was not in this run |
| stale_propagation | n/a | 0.95 | n/a | the boards retriever arrives at M6; memory was not in this run |
| answer_length_with_prior_context | n/a |  | n/a | the boards retriever arrives at M6; memory was not in this run |

## By provider

The reference sample is small on purpose. It is there to say whether the bulk provider is in the same league, not to be a score in its own right.

| Provider | Leg | Questions | Cards | Recall | Forbidden | Tokens | p50 latency |
| --- | --- | --- | --- | --- | --- | --- | --- |
| anthropic | reference | 9 | 9 | 0.222 | 0 | 19893 | 1363 ms |
| moonshot | bulk | 391 | 389 | 0.024 | 0 | 1346203 | 61309 ms |

## By edge case

| Case | Questions | Cards | Facts recalled |
| --- | --- | --- | --- |
| advice_bait | 20 | 20 | 0/20 |
| contradiction_across_classes | 8 | 8 | 0/8 |
| contradiction_within_class | 2 | 2 | 0/2 |
| empty_corpus | 20 | 20 | n/a |
| numeric_arithmetic_bait | 5 | 5 | 0/5 |
| partial_values | 4 | 4 | 0/4 |
| superseded_regulation | 192 | 192 | 7/192 |

## Verdict

1 metric(s) below threshold: domain_accuracy.
