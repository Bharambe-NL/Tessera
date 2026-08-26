#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The ancestor chain. Doc 03 section 4's parent block and doc 04 section 4's
//! ancestors.
//!
//! This exists because it was missing. Every card was built with `parent: null`
//! and `ancestors: []`, so a follow-up reached the retrievers as a question with
//! no subject: "which article says so?" names nothing to look for. Measured
//! through the pipeline, retrieval recall on standalone questions was 1.000 and
//! on follow-ups 0.485, and the whole of that gap was this.

use rusqlite::params;
use tessera_store::repo::{self, NewCard};
use tessera_store::{Store, new_id, now_iso8601};

struct Fixture {
    store: Store,
    board: String,
}

fn fixture() -> Fixture {
    let mut store = Store::open_in_memory().expect("store");
    let now = now_iso8601();
    let (profile, pack) = (new_id(), new_id());

    store
        .conn()
        .execute(
            "INSERT INTO doctrine_pack (id, code, version, audiences, source_hierarchy,
                 freshness_classes, flag_rules, retrievers, exercise_templates, created_at)
             VALUES (?1, 'general', '1.0', '[]', '[]', '[]', '[]', '[]', '[]', ?2)",
            params![pack, now],
        )
        .expect("pack");
    store
        .conn()
        .execute(
            "INSERT INTO profile (id, default_depth, default_doctrine_pack_id, model_policy,
                 retriever_config, created_at, updated_at)
             VALUES (?1, 'deep', ?2, '{}', '{}', ?3, ?3)",
            params![profile, pack, now],
        )
        .expect("profile");

    let board = repo::create_board(
        &mut store,
        repo::NewBoard {
            profile_id: &profile,
            title: "A board",
            doctrine_pack_id: &pack,
            default_depth: "deep",
            named_by_user: false,
            parent_board_id: None,
            seed_label: None,
            context: None,
        },
    )
    .expect("board");

    Fixture { store, board }
}

fn card(f: &mut Fixture, question: &str, parent: Option<&str>) -> String {
    repo::create_card(
        &mut f.store,
        NewCard {
            board_id: &f.board.clone(),
            parent_card_id: parent,
            kind: if parent.is_some() { "follow" } else { "root" },
            question,
            depth: "deep",
            anchor_text: None,
            anchor_block_ref: None,
            audience_id: None,
        },
    )
    .expect("card")
}

fn answer(f: &mut Fixture, card_id: &str, text: &str) {
    f.store
        .conn()
        .execute(
            "UPDATE card SET answer = ?2, status = 'done', confidence = 0.8 WHERE id = ?1",
            params![card_id, text],
        )
        .expect("answer");
}

#[test]
fn a_root_card_has_no_ancestors() {
    let mut f = fixture();
    let root = card(&mut f, "What is the buffer?", None);
    let chain = repo::ancestor_chain(&f.store, &root, 3).expect("chain");
    assert!(chain.is_empty(), "a root card invented an ancestor");
}

#[test]
fn a_follow_up_reaches_its_parents_question_and_answer() {
    // The whole point. Without this the Planner has nothing to resolve
    // "which article says so?" against.
    let mut f = fixture();
    let root = card(&mut f, "What is the confidence level for the internal model?", None);
    answer(&mut f, &root, "The confidence level is 98.4 percent.");
    let follow = card(&mut f, "Which article says so?", Some(&root));

    let chain = repo::ancestor_chain(&f.store, &follow, 3).expect("chain");
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].question, "What is the confidence level for the internal model?");
    assert_eq!(chain[0].answer.as_deref(), Some("The confidence level is 98.4 percent."));
}

#[test]
fn the_chain_runs_nearest_first() {
    // The Router wants the immediate parent and takes `first()`. Ordered the
    // other way it would read the oldest card on the thread as the parent.
    let mut f = fixture();
    let a = card(&mut f, "one", None);
    let b = card(&mut f, "two", Some(&a));
    let c = card(&mut f, "three", Some(&b));

    let chain = repo::ancestor_chain(&f.store, &c, 3).expect("chain");
    let questions: Vec<&str> = chain.iter().map(|a| a.question.as_str()).collect();
    assert_eq!(questions, vec!["two", "one"]);
}

#[test]
fn the_chain_stops_at_the_limit() {
    // Doc 04 section 4 caps ancestors at three, and the schema enforces it. An
    // unbounded walk on a long thread would put the whole board in a prompt.
    let mut f = fixture();
    let mut previous = card(&mut f, "root", None);
    for i in 0..6 {
        previous = card(&mut f, &format!("follow {i}"), Some(&previous));
    }

    let chain = repo::ancestor_chain(&f.store, &previous, 3).expect("chain");
    assert_eq!(chain.len(), 3);
}

#[test]
fn a_cycle_does_not_hang_the_walk() {
    // `parent_card_id` is a plain foreign key and nothing stops it pointing at
    // a descendant. A corrupt row should degrade, not spin.
    let mut f = fixture();
    let a = card(&mut f, "one", None);
    let b = card(&mut f, "two", Some(&a));
    f.store
        .conn()
        .execute("UPDATE card SET parent_card_id = ?2 WHERE id = ?1", params![&a, &b])
        .expect("cycle");

    let chain = repo::ancestor_chain(&f.store, &b, 3).expect("chain");
    assert!(chain.len() <= 3, "the walk did not stop");
}

#[test]
fn an_unanswered_parent_reports_no_answered_at() {
    // A timestamp on a card that never answered reads as freshness it does not
    // have, and the Router's staleness check runs off exactly that field.
    let mut f = fixture();
    let root = card(&mut f, "one", None);
    let follow = card(&mut f, "two", Some(&root));

    let chain = repo::ancestor_chain(&f.store, &follow, 3).expect("chain");
    assert_eq!(chain[0].answered_at, None);
    assert_eq!(chain[0].answer, None);

    answer(&mut f, &root, "Something.");
    let chain = repo::ancestor_chain(&f.store, &follow, 3).expect("chain");
    assert!(chain[0].answered_at.is_some(), "an answered card withheld its timestamp");
}

#[test]
fn a_missing_card_yields_an_empty_chain() {
    let f = fixture();
    let chain = repo::ancestor_chain(&f.store, "no-such-card", 3).expect("chain");
    assert!(chain.is_empty());
}
