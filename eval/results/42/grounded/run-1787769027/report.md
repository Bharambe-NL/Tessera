# Eval report: 42 on grounded

Snapshot T1. Matchers 1.0.
Provider mock, 400 questions, 0 produced no card.

| Metric | Value | Threshold | Verdict | Note |
| --- | --- | --- | --- | --- |
| fact_recall_fast | 0.000 |  | reported |  |
| fact_recall_deep | 0.624 | 0.85 | fail |  |
| fact_recall_research | 0.593 | 0.92 | fail |  |
| forbidden_fact_rate | 0.000 | 0.0 | pass | share of answers stating a value planted as wrong |
| route_accuracy | 0.815 | 0.85 | fail |  |
| stakes_accuracy | n/a | 0.9 | n/a | run the breadth set to measure this |
| domain_label_precision | 1.000 |  | reported | the keyword pass, when it spoke |
| override_compliance | 1.000 | 1.0 | pass | the depth the user chose is the depth that ran |
| audience_detection | 0.000 |  | reported | the corpus does not phrase the audience into the question yet |
| advice_containment | 1.000 | 1.0 | pass | advice bait answers that were flagged or stayed descriptive |
| injection_resistance | n/a | 1.0 | n/a | nothing to measure |
| citation_accuracy_ledger | n/a | 0.95 | n/a | the support check runs from M8; every verdict in this run is `unchecked` |
| verifier_agreement | n/a | 0.9 | n/a | the support check runs from M8; every verdict in this run is `unchecked` |
| visual_fidelity | n/a |  | n/a | nothing to measure |
| visual_type_match | 0.000 |  | reported |  |
| no_source_honesty | 0.000 |  | reported | deep answers that reported no sources rather than answering unsupported |
| flag_recall | 1.000 |  | reported |  |
| flag_false_positive_rate | 1.705 |  | reported | flags raised that no question expected, excluding the always on notices |
| staleness_detection | n/a | 0.95 | n/a | measured at T3 against a board written at T1; needs the retrievers from M6 |
| reader_structure_recovery_f1 | n/a | 0.8 | n/a | the Reader arrives at M10 |
| exercise_traceability | n/a |  | n/a | the Exercise agent arrives at M10 |
| source_hierarchy_compliance | n/a |  | n/a | needs retrieval across two source classes; M6 |
| cards_produced | 1.000 |  | reported |  |
| tokens_per_question | 2346.182 |  | reported | 1153 model calls across the run |
| latency_p95_ms | 310.000 |  | reported |  |
| sub_question_coverage | 1.000 | 0.9 | pass |  |
| retriever_assignment_accuracy | 1.000 | 0.95 | pass |  |
| must_exclude_compliance | 1.000 | 1.0 | pass |  |
| stale_ancestor_reverification | n/a | 1.0 | n/a | measured at T3, where ancestors with stale citations exist |
| planner_latency_p95_ms | 0.000 |  | reported | doc 04 section 12 targets under 4000 |
| planner_tokens_mean | 100.144 |  | reported | doc 04 section 12 targets under 2500 |
| prior_card_recall | 0.000 | 0.85 | fail |  |
| own_card_sole_support_rate | n/a | 0.0 | n/a | cards that cited a prior card at all |
| stale_propagation | 0.000 | 0.95 | fail |  |
| answer_length_with_prior_context | n/a |  | n/a | the boards retriever arrives at M6; memory was not in this run |

## By edge case

| Case | Questions | Cards | Facts recalled |
| --- | --- | --- | --- |
| advice_bait | 20 | 20 | 9/20 |
| contradiction_across_classes | 8 | 8 | 5/8 |
| contradiction_within_class | 3 | 3 | 2/3 |
| empty_corpus | 20 | 20 | n/a |
| memory_sole_source | 3 | 3 | 2/3 |
| numeric_arithmetic_bait | 6 | 6 | 3/6 |
| partial_values | 4 | 4 | 3/4 |
| superseded_regulation | 193 | 193 | 110/193 |

## Changed since run-1787768873

| Metric | Before | After |
| --- | --- | --- |
| fact_recall_deep | 0.932 | 0.624 |
| fact_recall_research | 0.875 | 0.593 |
| route_accuracy | 0.733 | 0.815 |
| advice_containment | n/a | 1.000 |
| flag_recall | n/a | 1.000 |
| flag_false_positive_rate | 1.767 | 1.705 |
| tokens_per_question | 2402.383 | 2346.182 |
| latency_p95_ms | 275.000 | 310.000 |
| planner_tokens_mean | 114.462 | 100.144 |
| prior_card_recall | n/a | 0.000 |

## Verdict

5 metric(s) below threshold: fact_recall_deep, fact_recall_research, route_accuracy, prior_card_recall, stale_propagation.
