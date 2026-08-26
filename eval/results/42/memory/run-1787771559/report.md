# Eval report: 42 on memory

Snapshot T1. Matchers 1.0.
Provider mock, 400 questions, 0 produced no card.

| Metric | Value | Threshold | Verdict | Note |
| --- | --- | --- | --- | --- |
| fact_recall_fast | 0.000 |  | reported |  |
| fact_recall_deep | 0.938 | 0.85 | pass |  |
| fact_recall_research | 1.000 | 0.92 | pass |  |
| fact_precision | 1.000 | 0.9 | pass | planted values stated that were the ones asked for |
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
| flag_false_positive_rate | 1.000 | 0.1 | reported | worst rule `unsupported_claim`; over threshold: injection_suspected, length_and_format, numeric_without_citation, unsupported_claim; advisory under a mock, which writes crudely and trips these by construction |
| staleness_detection | n/a | 0.95 | n/a | no citation in this run points at a superseded value; run the T3 snapshot |
| source_hierarchy_compliance | 1.000 |  | reported | answers that took the higher ranked value where two classes disagreed |
| reader_structure_recovery_f1 | n/a | 0.8 | n/a | the Reader arrives at M10; set reader_enabled when it does |
| exercise_traceability | n/a |  | n/a | the Exercise agent arrives at M10; set exercise_enabled when it does |
| cards_produced | 1.000 |  | reported |  |
| tokens_per_question | 3036.992 |  | reported | 1153 model calls across the run |
| latency_p95_ms | 471.000 |  | reported |  |
| sub_question_coverage | 1.000 | 0.9 | pass |  |
| retriever_assignment_accuracy | 1.000 | 0.95 | pass |  |
| must_exclude_compliance | 1.000 | 1.0 | pass |  |
| stale_ancestor_reverification | n/a | 1.0 | n/a | measured at T3, where ancestors with stale citations exist |
| planner_latency_p95_ms | 0.000 |  | reported | doc 04 section 12 targets under 4000 |
| planner_tokens_mean | 222.051 |  | reported | doc 04 section 12 targets under 2500 |
| prior_card_recall | 1.000 | 0.85 | pass |  |
| own_card_sole_support_rate | n/a | 0.0 | n/a | cards that cited a prior card at all |
| stale_propagation | n/a | 0.95 | n/a | no run re-verified a prior card; needs the verify_only run |
| answer_length_with_prior_context | n/a |  | n/a | no answer without prior context had any length to compare against |

## By edge case

| Case | Questions | Cards | Facts recalled |
| --- | --- | --- | --- |
| advice_bait | 20 | 20 | 9/20 |
| contradiction_across_classes | 8 | 8 | 8/8 |
| contradiction_within_class | 3 | 3 | 3/3 |
| empty_corpus | 20 | 20 | n/a |
| memory_sole_source | 3 | 3 | 3/3 |
| numeric_arithmetic_bait | 6 | 6 | 6/6 |
| partial_values | 4 | 4 | 4/4 |
| superseded_regulation | 193 | 193 | 173/193 |

## Verdict

1 metric(s) below threshold: route_accuracy.
