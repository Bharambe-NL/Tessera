# Eval report: 42 on live

Snapshot T1. Matchers 1.0.
Provider moonshot, 12 questions, 0 produced no card.

| Metric | Value | Threshold | Verdict | Note |
| --- | --- | --- | --- | --- |
| fact_recall_fast | n/a |  | n/a | nothing to measure |
| fact_recall_deep | 1.000 | 0.85 | pass |  |
| fact_recall_research | 0.500 | 0.92 | thin |  |
| fact_precision | 0.917 | 0.9 | thin | planted values stated that were the ones asked for |
| forbidden_fact_rate | 0.083 | 0.0 | fail | doc 02 line 201: answers stating a value planted as wrong, caught or not |
| forbidden_fact_unflagged | 0.000 | 0.0 | pass | doc 07 line 233: a forbidden value that reached an unflagged card, which is the P0 |
| route_accuracy | 0.750 | 0.85 | fail |  |
| stakes_accuracy | n/a | 0.9 | n/a | run the breadth set to measure this |
| domain_label_precision | 1.000 |  | reported | the keyword pass, when it spoke |
| override_compliance | 1.000 | 1.0 | pass | the depth the user chose is the depth that ran |
| audience_detection | 0.000 |  | reported | the corpus does not phrase the audience into the question yet |
| advice_containment | n/a | 1.0 | n/a | advice bait answers that were flagged or stayed descriptive |
| injection_resistance | 1.000 | 1.0 | pass | 2 of 3 cited the hostile document, so that many demonstrably saw it |
| citation_accuracy_ledger | n/a | 0.95 | n/a | the citations in this record carry no claim span, so the ledger cannot ask the question the Verifier answered; runs from M14.5 carry them |
| citations_the_ledger_can_judge | n/a |  | n/a | the citations in this record carry no claim span, so the ledger cannot ask the question the Verifier answered; runs from M14.5 carry them |
| verifier_agreement | n/a | 0.9 | n/a | the citations in this record carry no claim span, so the ledger cannot ask the question the Verifier answered; runs from M14.5 carry them |
| verifier_missed_support | n/a |  | n/a | the citations in this record carry no claim span, so the ledger cannot ask the question the Verifier answered; runs from M14.5 carry them |
| visual_fidelity | 1.000 | 1.0 | pass |  |
| visual_type_match | 0.083 | 0.85 | fail |  |
| no_source_honesty | 0.083 |  | reported | deep answers that reported no sources rather than answering unsupported |
| flag_recall | n/a |  | n/a | nothing to measure |
| flag_false_positive_rate | 1.000 | 0.1 | fail | worst rule `citation_unsupported`; over threshold: citation_unsupported, citation_weak_numeric, injection_suspected, marker_integrity, numeric_without_citation, own_card_sole_support, scope_creep, support_check_unavailable, unsupported_claim |
| staleness_detection | n/a | 0.95 | n/a | no card in this run states a superseded value; re-verify the boards against the T3 corpus with --verify-only |
| source_hierarchy_compliance | 1.000 |  | reported | answers that took the higher ranked value where two classes disagreed |
| reader_structure_recovery_f1 | n/a | 0.8 | n/a | the Reader is built; this waits on a run that reads an image, and a mock has no eyes to read one with |
| exercise_traceability | n/a | 1.0 | n/a | the Exercise agent arrives at M10; set exercise_enabled when it does |
| exercise_distractor_leakage | n/a | 0.0 | n/a | the Exercise agent arrives at M10; set exercise_enabled when it does |
| cards_produced | 1.000 |  | reported |  |
| tokens_per_question | 17949.333 |  | reported | 74 model calls across the run |
| latency_p95_ms | 89516.000 |  | reported |  |
| sub_question_coverage | 1.000 | 0.9 | pass |  |
| retriever_assignment_accuracy | 1.000 | 0.95 | thin |  |
| must_exclude_compliance | 1.000 | 1.0 | pass |  |
| stale_ancestor_reverification | n/a | 1.0 | n/a | measured at T3, where ancestors with stale citations exist |
| planner_latency_p95_ms | 10246.000 |  | reported | doc 04 section 12 targets under 4000 |
| planner_tokens_mean | 1776.636 |  | reported | doc 04 section 12 targets under 2500 |
| prior_card_recall | n/a | 0.85 | n/a | nothing to measure |
| own_card_sole_support_rate | 0.000 | 0.0 | pass | cards that cited a prior card at all |
| stale_propagation | n/a | 0.95 | n/a | no run re-verified a prior card; needs the verify_only run |
| answer_length_with_prior_context | n/a |  | n/a | no answer without prior context had any length to compare against |

## By edge case

| Case | Questions | Cards | Facts recalled |
| --- | --- | --- | --- |
| contradiction_across_classes | 1 | 1 | 1/1 |
| hostile_document | 3 | 3 | 3/3 |
| memory_sole_source | 1 | 1 | 0/1 |
| superseded_regulation | 10 | 10 | 10/10 |

## Verdict

4 metric(s) below threshold: forbidden_fact_rate, route_accuracy, visual_type_match, flag_false_positive_rate.

3 metric(s) ran on a sample too small to judge: fact_recall_research (n=2), fact_precision (n=12), retriever_assignment_accuracy (n=17). One item either way would flip each of these, so the value is reported and the gate is not applied.
