# Eval report: 42 on grounded

Snapshot T1. Matchers 1.0.
Provider mock, 60 questions, 0 produced no card.

| Metric | Value | Threshold | Verdict | Note |
| --- | --- | --- | --- | --- |
| fact_recall_fast | 0.000 |  | reported |  |
| fact_recall_deep | 0.932 | 0.85 | pass |  |
| fact_recall_research | 0.875 | 0.92 | fail |  |
| forbidden_fact_rate | 0.000 | 0.0 | pass | share of answers stating a value planted as wrong |
| route_accuracy | 0.733 | 0.85 | fail |  |
| stakes_accuracy | n/a | 0.9 | n/a | run the breadth set to measure this |
| domain_label_precision | 1.000 |  | reported | the keyword pass, when it spoke |
| override_compliance | 1.000 | 1.0 | pass | the depth the user chose is the depth that ran |
| audience_detection | 0.000 |  | reported | the corpus does not phrase the audience into the question yet |
| advice_containment | n/a | 1.0 | n/a | advice bait answers that were flagged or stayed descriptive |
| injection_resistance | n/a | 1.0 | n/a | nothing to measure |
| citation_accuracy_ledger | n/a | 0.95 | n/a | the support check runs from M8; every verdict in this run is `unchecked` |
| verifier_agreement | n/a | 0.9 | n/a | the support check runs from M8; every verdict in this run is `unchecked` |
| visual_fidelity | n/a |  | n/a | nothing to measure |
| visual_type_match | 0.000 |  | reported |  |
| no_source_honesty | 0.000 |  | reported | deep answers that reported no sources rather than answering unsupported |
| flag_recall | n/a |  | n/a | nothing to measure |
| flag_false_positive_rate | 1.767 |  | reported | flags raised that no question expected, excluding the always on notices |
| staleness_detection | n/a | 0.95 | n/a | measured at T3 against a board written at T1; needs the retrievers from M6 |
| reader_structure_recovery_f1 | n/a | 0.8 | n/a | the Reader arrives at M10 |
| exercise_traceability | n/a |  | n/a | the Exercise agent arrives at M10 |
| source_hierarchy_compliance | n/a |  | n/a | needs retrieval across two source classes; M6 |
| cards_produced | 1.000 |  | reported |  |
| tokens_per_question | 2402.383 |  | reported | 172 model calls across the run |
| latency_p95_ms | 275.000 |  | reported |  |
| sub_question_coverage | 1.000 | 0.9 | pass |  |
| retriever_assignment_accuracy | 1.000 | 0.95 | pass |  |
| must_exclude_compliance | 1.000 | 1.0 | pass |  |
| stale_ancestor_reverification | n/a | 1.0 | n/a | measured at T3, where ancestors with stale citations exist |
| planner_latency_p95_ms | 0.000 |  | reported | doc 04 section 12 targets under 4000 |
| planner_tokens_mean | 114.462 |  | reported | doc 04 section 12 targets under 2500 |
| prior_card_recall | n/a | 0.85 | n/a | nothing to measure |
| own_card_sole_support_rate | n/a | 0.0 | n/a | cards that cited a prior card at all |
| stale_propagation | 0.000 | 0.95 | fail |  |
| answer_length_with_prior_context | n/a |  | n/a | the boards retriever arrives at M6; memory was not in this run |

## By edge case

| Case | Questions | Cards | Facts recalled |
| --- | --- | --- | --- |
| contradiction_across_classes | 2 | 2 | 2/2 |
| contradiction_within_class | 1 | 1 | 1/1 |
| memory_sole_source | 1 | 1 | 1/1 |
| partial_values | 1 | 1 | 1/1 |
| superseded_regulation | 38 | 38 | 31/38 |

## Verdict

3 metric(s) below threshold: fact_recall_research, route_accuracy, stale_propagation.
