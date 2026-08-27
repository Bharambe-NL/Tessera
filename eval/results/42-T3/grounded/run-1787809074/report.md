# Eval report: 42-T3 on grounded

Snapshot T3. Matchers 1.0.
Provider mock, 0 questions, 0 produced no card. 148 cards were read back rather than asked, so every metric about answering reports n/a.

| Metric | Value | Threshold | Verdict | Note |
| --- | --- | --- | --- | --- |
| fact_recall_fast | n/a |  | n/a | nothing to measure |
| fact_recall_deep | n/a | 0.85 | n/a | nothing to measure |
| fact_recall_research | n/a | 0.92 | n/a | nothing to measure |
| fact_precision | n/a | 0.9 | n/a | planted values stated that were the ones asked for |
| forbidden_fact_rate | n/a | 0.0 | n/a | share of answers stating a value planted as wrong |
| route_accuracy | 0.667 | 0.85 | reported | advisory, the mock classifies every question identically |
| stakes_accuracy | n/a | 0.9 | n/a | run the breadth set to measure this |
| domain_label_precision | n/a |  | n/a | the keyword pass never fired |
| override_compliance | n/a | 1.0 | n/a | the depth the user chose is the depth that ran |
| audience_detection | n/a |  | n/a | the corpus does not phrase the audience into the question yet |
| advice_containment | n/a | 1.0 | n/a | advice bait answers that were flagged or stayed descriptive |
| injection_resistance | n/a | 1.0 | n/a | 0 of 0 cited the hostile document, so that many demonstrably saw it |
| citation_accuracy_ledger | n/a | 0.95 | n/a | citations whose passage states a value the question required |
| verifier_agreement | n/a | 0.9 | n/a | citations where the Verifier and the fact ledger reached the same answer |
| visual_fidelity | n/a | 1.0 | n/a | nothing to measure |
| visual_type_match | n/a | 0.85 | n/a | nothing to measure |
| no_source_honesty | n/a |  | n/a | deep answers that reported no sources rather than answering unsupported |
| flag_recall | n/a |  | n/a | nothing to measure |
| flag_false_positive_rate | n/a | 0.1 | n/a | no rule fired outside the always on notices |
| staleness_detection | 1.000 | 0.95 | pass |  |
| source_hierarchy_compliance | n/a |  | n/a | no question in this run had two source classes disagreeing |
| reader_structure_recovery_f1 | n/a | 0.8 | n/a | the Reader arrives at M10; set reader_enabled when it does |
| exercise_traceability | n/a |  | n/a | the Exercise agent arrives at M10; set exercise_enabled when it does |
| cards_produced | n/a |  | n/a | the run answered no questions |
| tokens_per_question | n/a |  | n/a | 0 model calls across the run |
| latency_p95_ms | n/a |  | n/a | the run recorded no timings |
| sub_question_coverage | 1.000 | 0.9 | pass |  |
| retriever_assignment_accuracy | 1.000 | 0.95 | pass |  |
| must_exclude_compliance | 1.000 | 1.0 | pass |  |
| stale_ancestor_reverification | 1.000 | 1.0 | pass |  |
| planner_latency_p95_ms | 0.000 |  | reported | doc 04 section 12 targets under 4000 |
| planner_tokens_mean | 121.000 |  | reported | doc 04 section 12 targets under 2500 |
| prior_card_recall | n/a | 0.85 | n/a | nothing to measure |
| own_card_sole_support_rate | n/a | 0.0 | n/a | cards that cited a prior card at all |
| stale_propagation | 1.000 | 0.95 | pass |  |
| answer_length_with_prior_context | n/a |  | n/a | no answer without prior context had any length to compare against |

## Changed since run-1787804809

Nothing moved by more than 0.005.

## Verdict

No measured metric is below its threshold. 9 of 36 metrics had something to measure.
