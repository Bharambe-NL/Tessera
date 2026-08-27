//! The UI bridge. Pattern 25.
//!
//! Doc 10 section 5 lists the event bus subscribers and describes this one as
//! the thing "which translates events into a small set of UI notifications,
//! Pattern 25's projection discipline".
//!
//! The discipline is the point. The event log has around sixty event types; the
//! UI needs a handful. Translating here rather than shipping raw events to the
//! webview means the protocol is a *view* over the log, so the log can gain an
//! event type without the frontend learning about it, and the frontend cannot
//! come to depend on the log's shape.
//!
//! Doc 09 section 4 fixes the streaming states this produces: "Routing (only if
//! over 400 ms), Planning, Searching {retriever names}, Answering, Building the
//! visual, Verifying."

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tessera_store::Event;

/// The whole notification vocabulary the webview knows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Notification {
    /// One line in a card's streaming stage list.
    CardStage {
        card_id: String,
        /// House style: a verb naming what is happening. Doc 11 section 9.
        label: String,
        done: bool,
    },
    /// The card's content changed and the canvas should re-read it.
    CardUpdated {
        card_id: String,
    },
    /// Terminal. `status` is `done` or `flagged`.
    CardAnswered {
        card_id: String,
        status: String,
        confidence: Option<f64>,
    },
    CardFailed {
        card_id: String,
        reason: String,
    },
    /// A flag exists. The rail's Flags count and the card's chip read this.
    FlagRaised {
        card_id: String,
        rule_id: String,
        severity: String,
    },
    FlagResolved {
        card_id: String,
    },
    BoardUpdated {
        board_id: String,
    },
    /// Doc 09 section 8: a toast when a verify_only run flags cards on the open
    /// board, a Profile notice when a key fails or a corpus update lands.
    Toast {
        level: ToastLevel,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToastLevel {
    Info,
    Warn,
    Error,
}

fn card_id(ev: &Event) -> Option<String> {
    ev.card_id.clone().or_else(|| {
        ev.payload
            .get("card_id")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn stage(ev: &Event, label: &str, done: bool) -> Option<Notification> {
    Some(Notification::CardStage {
        card_id: card_id(ev)?,
        label: label.to_string(),
        done,
    })
}

/// Translate one event. `None` means the UI does not need to hear about it,
/// which is true of most of the vocabulary: `citation.bound.v1` and
/// `model.call.v1` matter to the audit trail and to cost, not to the canvas.
pub fn translate(ev: &Event) -> Option<Notification> {
    match ev.event_type.as_str() {
        // ------------------------------------------------- streaming stages --
        // Doc 03 section 13: the UI shows "Routing…" only if the Router takes
        // longer than 400 ms, to avoid a flicker on fast routes. The bridge
        // always emits it; the frontend holds it back.
        "card.requested.v1" => stage(ev, "Routing", false),
        "card.routed.v1" => {
            let next = if ev.payload.get("plan_required").and_then(Value::as_bool) == Some(true) {
                "Planning"
            } else {
                "Answering"
            };
            stage(ev, next, false)
        }
        "card.planned.v1" => stage(ev, "Planning", true),
        "retrieval.started.v1" => {
            let retriever = ev
                .payload
                .get("retriever_id")
                .and_then(Value::as_str)
                .unwrap_or("sources");
            stage(ev, &format!("Searching {retriever}"), false)
        }
        "retrieval.completed.v1" => {
            let retriever = ev
                .payload
                .get("retriever_id")
                .and_then(Value::as_str)
                .unwrap_or("sources");
            stage(ev, &format!("Searching {retriever}"), true)
        }
        "card.synthesized.v1" => stage(ev, "Answering", true),
        "visual.produced.v1" | "visual.declined.v1" => stage(ev, "Building the visual", true),
        "verify.completed.v1" => stage(ev, "Verifying", true),

        // ------------------------------------------------------- terminal ----
        "card.answered.v1" => Some(Notification::CardAnswered {
            card_id: card_id(ev)?,
            status: ev
                .payload
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("done")
                .to_string(),
            confidence: ev.payload.get("card_confidence").and_then(Value::as_f64),
        }),
        "card.failed.v1" => Some(Notification::CardFailed {
            card_id: card_id(ev)?,
            reason: ev
                .payload
                .get("failure")
                .and_then(|f| f.get("detail"))
                .and_then(Value::as_str)
                .unwrap_or("This card did not finish.")
                .to_string(),
        }),

        // ---------------------------------------------------------- flags ----
        "flag.raised.v1" => Some(Notification::FlagRaised {
            card_id: card_id(ev)?,
            rule_id: ev
                .payload
                .get("rule_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            severity: ev
                .payload
                .get("severity")
                .and_then(Value::as_str)
                .unwrap_or("info")
                .to_string(),
        }),
        "review.decided.v1" => Some(Notification::FlagResolved {
            card_id: card_id(ev)?,
        }),
        "card.blocked.v1" => Some(Notification::CardUpdated {
            card_id: card_id(ev)?,
        }),

        // -------------------------------------------------------- content ----
        "card.superseded.v1" | "read.completed.v1" => Some(Notification::CardUpdated {
            card_id: card_id(ev)?,
        }),

        // ---------------------------------------------------------- board ----
        "board.created.v1" | "board.renamed.v1" | "board.trashed.v1" | "board.restored.v1"
        | "board.imported.v1" | "ink.added.v1" | "ink.erased.v1" | "note.added.v1" | "note.edited.v1"
        | "note.removed.v1" | "image.pasted.v1" | "image.generated.v1" => Some(Notification::BoardUpdated {
            board_id: ev.board_id.clone()?,
        }),

        // --------------------------------------------------------- notices ----
        // Doc 07 section B14 open question 2, resolved as proposed: a batch of
        // stale flags is one item, not one per card.
        "source.stale.v1" => Some(Notification::Toast {
            level: ToastLevel::Warn,
            message: match ev.payload.get("affected_cards").and_then(Value::as_u64) {
                Some(n) if n > 1 => format!("A source went stale. It affects {n} cards."),
                _ => "A source went stale.".to_string(),
            },
        }),
        "hook.denied.v1" => Some(Notification::Toast {
            level: ToastLevel::Info,
            message: format!(
                "A retriever skipped {}.",
                ev.payload
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or("an excluded item")
            ),
        }),
        "model.fallback.v1" => Some(Notification::Toast {
            level: ToastLevel::Warn,
            message: "A model was unavailable, so a fallback ran instead.".to_string(),
        }),

        _ => None,
    }
}

/// Translate a batch, dropping what the UI does not need.
pub fn translate_all(events: &[Event]) -> Vec<Notification> {
    events.iter().filter_map(translate).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tessera_store::{EmitterType, Provenance, Source, TrustLevel};

    fn event(event_type: &str, payload: Value) -> Event {
        Event {
            event_id: "01JAV9YQ4M8T7R2K5N6P3W1XZQ".into(),
            monotonic_index: 1,
            event_type: event_type.into(),
            payload,
            provenance: Provenance {
                source: Source::Live,
                emitter_id: "harness".into(),
                emitter_type: EmitterType::Harness,
                run_id: None,
                trust_level: TrustLevel::Verified,
            },
            causal_parent_id: None,
            board_id: Some("board-1".into()),
            card_id: Some("card-1".into()),
            timestamp: "2026-08-25T09:00:00.000Z".into(),
        }
    }

    #[test]
    fn a_planned_route_announces_planning_and_a_fast_one_announces_answering() {
        // Doc 09 section 4: the stage list is derived from events, and fast mode
        // skips the Planner entirely (doc 04 section 1).
        let planned = translate(&event("card.routed.v1", json!({ "plan_required": true })));
        assert_eq!(
            planned,
            Some(Notification::CardStage {
                card_id: "card-1".into(),
                label: "Planning".into(),
                done: false
            })
        );

        let fast = translate(&event("card.routed.v1", json!({ "plan_required": false })));
        assert!(matches!(fast, Some(Notification::CardStage { label, .. }) if label == "Answering"));
    }

    #[test]
    fn a_search_stage_names_its_retriever() {
        let n = translate(&event(
            "retrieval.started.v1",
            json!({ "retriever_id": "regulatory" }),
        ));
        assert!(
            matches!(n, Some(Notification::CardStage { label, done: false, .. }) if label == "Searching regulatory")
        );
    }

    #[test]
    fn most_of_the_vocabulary_is_not_the_uis_business() {
        // Pattern 25: the protocol is a view over the log, not the log.
        for t in [
            "citation.bound.v1",
            "model.call.v1",
            "source.created.v1",
            "concept.proposed.v1",
            "index.updated.v1",
            "schema.violation.v1",
        ] {
            assert_eq!(
                translate(&event(t, json!({}))),
                None,
                "{t} should not reach the canvas"
            );
        }
    }

    #[test]
    fn a_batch_of_stale_cards_is_one_notice_not_five() {
        // Doc 07 section B14 open question 2 and doc 09 section 6.
        let n = translate(&event("source.stale.v1", json!({ "affected_cards": 5 })));
        assert_eq!(
            n,
            Some(Notification::Toast {
                level: ToastLevel::Warn,
                message: "A source went stale. It affects 5 cards.".into()
            })
        );
    }

    #[test]
    fn a_hook_denial_notice_names_the_category_and_not_the_item() {
        // Doc 05 section 10, carried through to the surface.
        let n = translate(&event(
            "hook.denied.v1",
            json!({ "category": "an excluded folder", "target": "Sensitive/merger.docx" }),
        ));
        let Some(Notification::Toast { message, .. }) = n else {
            panic!("expected a toast");
        };
        assert!(message.contains("an excluded folder"));
        assert!(
            !message.contains("merger"),
            "the notice must not leak the filename"
        );
    }

    #[test]
    fn an_answered_card_carries_its_status_and_confidence() {
        let n = translate(&event(
            "card.answered.v1",
            json!({ "status": "flagged", "card_confidence": 0.41 }),
        ));
        assert_eq!(
            n,
            Some(Notification::CardAnswered {
                card_id: "card-1".into(),
                status: "flagged".into(),
                confidence: Some(0.41)
            })
        );
    }

    #[test]
    fn notifications_serialise_with_a_kind_tag() {
        // The webview switches on `kind`, so it has to be on the wire.
        let n = Notification::CardStage {
            card_id: "c".into(),
            label: "Verifying".into(),
            done: false,
        };
        let v = serde_json::to_value(&n).expect("serialise");
        assert_eq!(v["kind"], "card_stage");
        assert_eq!(v["label"], "Verifying");
    }

    #[test]
    fn a_card_id_in_the_payload_is_used_when_the_envelope_has_none() {
        let mut ev = event(
            "flag.raised.v1",
            json!({ "card_id": "card-9", "rule_id": "advice_language", "severity": "warn" }),
        );
        ev.card_id = None;
        assert_eq!(
            translate(&ev),
            Some(Notification::FlagRaised {
                card_id: "card-9".into(),
                rule_id: "advice_language".into(),
                severity: "warn".into()
            })
        );
    }
}
