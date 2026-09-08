//! The registered method surface, one module per noun.
//!
//! Doc 10 section 2: one core, several shells. Everything the desktop shell can
//! do is a method registered here, so the web client that arrives later talks to
//! the identical protocol over a socket rather than to a second, thinner API.
//!
//! The handlers were one function in `core.rs` until it reached eighteen hundred
//! lines and sixty nine verbs, at which point finding the one that answers a
//! question meant scrolling past sixty eight that did not. They are grouped by
//! the noun the verb names, because that is how a reader arrives: with a verb in
//! hand from the network log or the UI, wanting the code behind it.
//!
//! The directory is `verbs` rather than `handlers` because `rpc` next door is
//! already the transport and its `Handler` type; a `handlers` module beside it
//! would read as the other half of the transport rather than as the product's
//! own surface.

use crate::core::Core;
use crate::rpc::Router;

mod boards;
mod bundles;
mod cards;
mod exercises;
mod flags;
mod learn;
mod library;
mod map;
mod notebook;
mod notes;
mod pages;
mod profile;
mod support;

/// Register every method the shell may call. Doc 10 section 2's boundary.
pub fn build_router() -> Router<Core> {
    let mut r = Router::new();

    boards::register(&mut r);
    cards::register(&mut r);
    notes::register(&mut r);
    flags::register(&mut r);
    library::register(&mut r);
    learn::register(&mut r);
    exercises::register(&mut r);
    notebook::register(&mut r);
    pages::register(&mut r);
    map::register(&mut r);
    bundles::register(&mut r);
    profile::register(&mut r);

    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every verb the shell may call, in the order a `BTreeMap` hands them back.
    ///
    /// The point of the list is the split that produced it: twelve modules each
    /// register their own, and a `register` call dropped in a move would take a
    /// verb off the surface with nothing else failing. This is what fails.
    const SURFACE: [&str; 69] = [
        "board.add_image",
        "board.create",
        "board.export",
        "board.export_preflight",
        "board.get",
        "board.history",
        "board.import",
        "board.list",
        "board.notifications",
        "board.purge",
        "board.rasterise_ink",
        "board.rename",
        "board.restore",
        "board.trash",
        "board.update_pack",
        "card.ask",
        "card.move",
        "card.read",
        "card.save_as_page",
        "card.verify",
        "card.viewed",
        "concept.confirm_edge",
        "concept.decide",
        "concept.rate",
        "exercise.attempt",
        "exercise.create",
        "exercise.list",
        "exercise.report_item",
        "flag.decide",
        "flag.list",
        "learn.answer_check",
        "learn.answer_intake",
        "learn.build",
        "learn.check",
        "learn.end",
        "learn.get",
        "learn.say",
        "learn.start",
        "learning.plan",
        "library.concepts",
        "library.sources",
        "map.concept",
        "map.read",
        "mission.create",
        "mission.summary",
        "note.create",
        "note.edit",
        "note.remove",
        "notebook.open",
        "notebook.open_on_board",
        "notebook.search_web",
        "notebook.session",
        "notebook.sessions",
        "pack.import",
        "page.create_from_link",
        "page.delete",
        "page.get",
        "page.list",
        "page.write",
        "path.load",
        "profile.back_up",
        "profile.diagnostics",
        "profile.first_run",
        "profile.get",
        "profile.set_key",
        "profile.set_pack",
        "profile.watch_folder",
        "profile.watch_web",
        "vault.sync",
    ];

    #[test]
    fn the_surface_is_the_sixty_nine_verbs_and_no_others() {
        let router = build_router();
        let registered: Vec<&str> = router.methods().collect();
        assert_eq!(
            registered.len(),
            SURFACE.len(),
            "a register call was dropped or added in a move"
        );
        assert_eq!(registered, SURFACE.to_vec());
    }
}
