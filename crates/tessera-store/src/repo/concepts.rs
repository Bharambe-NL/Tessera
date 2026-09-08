//! Concepts, the edges between them, and the mission a lesson is planned for.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::cards::CardRef;
use crate::error::Result;
use crate::event::{NewEvent, Provenance};
use crate::{Store, new_id, now_iso8601};

/// Propose the concepts a card named, and link them to it.
///
/// Doc 01 section 4.10: "Agents propose; the user confirms." The Router already
/// returns the entities a question names, and until M9 they went into the log
/// and nowhere else, so the Planner packet's `concepts` was an empty array and
/// entity resolution degraded to literals marked `unknown` exactly as doc 04
/// says it should when the graph is empty.
///
/// A term the profile already knows is reused rather than duplicated, which is
/// what doc 01 section 4.11 means by "two boards that both cite the same Concept
/// share it". Matching is case insensitive on the canonical spelling; an alias
/// pass belongs with the Concept editor, not here.
///
/// Returns how many concepts were newly proposed.
pub fn propose_concepts(
    store: &mut Store,
    at: CardRef<'_>,
    profile_id: &str,
    doctrine_pack_id: &str,
    terms: &[String],
    proposed_by: &str,
) -> Result<usize> {
    let mut proposed = 0usize;

    for term in terms {
        let term = term.trim();
        // A one character entity is noise, and an empty one is a model slip.
        if term.chars().count() < 2 || term.chars().count() > 120 {
            continue;
        }

        let existing: Option<String> = store
            .conn()
            .query_row(
                "SELECT id FROM concept WHERE profile_id = ?1 AND lower(term) = lower(?2)",
                params![profile_id, term],
                |r| r.get(0),
            )
            .optional()?;

        let concept_id = match existing {
            Some(id) => {
                // Doc 01 section 6.3's `entity.resolved.v1`: the entity named a
                // node the profile already has, which is the whole point of the
                // graph being shared across boards.
                store.append(
                    NewEvent::new(
                        "entity.resolved.v1",
                        json!({ "concept_id": id, "term": term, "card_id": at.card_id }),
                        Provenance::agent(proposed_by, at.run_id.to_string()),
                    )
                    .on_board(at.board_id)
                    .on_card(at.card_id),
                )?;
                id
            }
            None => {
                let id = new_id();
                let now = now_iso8601();
                let (row, profile, pack, name, at_time) = (
                    id.clone(),
                    profile_id.to_string(),
                    doctrine_pack_id.to_string(),
                    term.to_string(),
                    now,
                );
                store.append_with(
                    NewEvent::new(
                        "concept.proposed.v1",
                        json!({ "concept_id": id, "term": term, "card_id": at.card_id }),
                        Provenance::agent(proposed_by, at.run_id.to_string()),
                    )
                    .on_board(at.board_id)
                    .on_card(at.card_id),
                    move |tx| {
                        tx.execute(
                            "INSERT INTO concept (id, profile_id, term, doctrine_pack_id, status,
                                                  created_at, updated_at)
                             VALUES (?1, ?2, ?3, ?4, 'proposed', ?5, ?5)",
                            params![row, profile, name, pack, at_time],
                        )?;
                        Ok(())
                    },
                )?;
                proposed += 1;
                id
            }
        };

        // One link per concept per card. A card asked twice about the same term
        // should touch the node once.
        let linked: i64 = store.conn().query_row(
            "SELECT COUNT(*) FROM concept_link
             WHERE concept_id = ?1 AND target_type = 'card' AND target_ref = ?2",
            params![concept_id, at.card_id],
            |r| r.get(0),
        )?;
        if linked > 0 {
            continue;
        }

        let link_id = new_id();
        let (row, cid, card, by, now) = (
            link_id.clone(),
            concept_id.clone(),
            at.card_id.to_string(),
            json!({ "agent_id": proposed_by }).to_string(),
            now_iso8601(),
        );
        store.append_with(
            NewEvent::new(
                "concept.linked.v1",
                json!({
                    "link_id": link_id,
                    "concept_id": concept_id,
                    "target_type": "card",
                    "target_ref": at.card_id,
                    "relation": "mentions",
                }),
                Provenance::agent(proposed_by, at.run_id.to_string()),
            )
            .on_board(at.board_id)
            .on_card(at.card_id),
            move |tx| {
                tx.execute(
                    "INSERT INTO concept_link (id, concept_id, target_type, target_ref, relation,
                                               proposed_by, status, created_at)
                     VALUES (?1, ?2, 'card', ?3, 'mentions', ?4, 'proposed', ?5)",
                    params![row, cid, card, by, now],
                )?;
                Ok(())
            },
        )?;
    }

    Ok(proposed)
}

/// Confirm or reject a proposed concept. Doc 09 section 9's row actions.
///
/// `None` means no proposed concept had that id, so nothing was decided and the
/// caller can say so rather than reporting a decision that never happened.
pub fn decide_concept(store: &mut Store, concept_id: &str, accept: bool) -> Result<Option<String>> {
    let term: Option<String> = store
        .conn()
        .query_row(
            "SELECT term FROM concept WHERE id = ?1 AND status = 'proposed'",
            params![concept_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(term) = term else { return Ok(None) };

    let (id, now) = (concept_id.to_string(), now_iso8601());
    if accept {
        store.append_with(
            NewEvent::new(
                "concept.confirmed.v1",
                json!({ "concept_id": concept_id, "term": term }),
                Provenance::user(),
            ),
            move |tx| {
                tx.execute(
                    "UPDATE concept SET status = 'confirmed', updated_at = ?1 WHERE id = ?2",
                    params![now, id],
                )?;
                Ok(())
            },
        )?;
    } else {
        // A rejected concept leaves, and its links leave with it. Doc 01 section
        // 4.11: links are how boards touch a node, so a node nobody kept has
        // nothing left to touch. There is no `concept.rejected.v1` in the
        // vocabulary and this does not invent one: the link rows carry the
        // status the model has for a rejection, and `concept.linked.v1` already
        // said they existed.
        store.append_with(
            NewEvent::new(
                "concept.linked.v1",
                json!({ "concept_id": concept_id, "term": term, "status": "rejected" }),
                Provenance::user(),
            ),
            move |tx| {
                tx.execute(
                    "UPDATE concept_link SET status = 'rejected' WHERE concept_id = ?1",
                    params![id],
                )?;
                Ok(())
            },
        )?;
    }
    Ok(Some(term))
}

/// The concepts the Planner packet carries. Doc 04 section 4.
///
/// Confirmed terms first, because those are the ones a person has stood behind,
/// then proposed ones, so a fresh profile still gets the graph it has.
pub fn concepts_for_packet(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT id, term, definition, status FROM concept
         WHERE profile_id = ?1
         ORDER BY CASE status WHEN 'confirmed' THEN 0 ELSE 1 END, term
         LIMIT ?2",
    )?;
    Ok(stmt
        .query_map(params![profile_id, limit], |r| {
            Ok(json!({
                "concept_id": r.get::<_, String>(0)?,
                "term": r.get::<_, String>(1)?,
                "definition": r.get::<_, Option<String>>(2)?,
                "status": r.get::<_, String>(3)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Library, Concepts tab. Doc 09 section 9.
///
/// "term, status (proposed or confirmed), definition, audience definitions,
/// linked cards". The link count is what doc 09 section 5's Remove on a concept
/// checks: only if unlinked.
pub fn list_concepts(store: &Store, profile_id: &str, limit: i64) -> Result<Vec<Value>> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT c.id, c.term, c.status, c.definition, c.aliases, c.audience_definitions,
                c.definition_card_id, c.updated_at,
                (SELECT COUNT(*) FROM concept_link l
                 WHERE l.concept_id = c.id AND l.status != 'rejected') AS links
         FROM concept c
         WHERE c.profile_id = ?1
         ORDER BY CASE c.status WHEN 'proposed' THEN 0 ELSE 1 END, c.term
         LIMIT ?2",
    )?;
    Ok(stmt
        .query_map(params![profile_id, limit], |r| {
            let json_col = |v: Option<String>| {
                v.and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .unwrap_or(Value::Null)
            };
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "term": r.get::<_, String>(1)?,
                "status": r.get::<_, String>(2)?,
                "definition": r.get::<_, Option<String>>(3)?,
                "aliases": json_col(r.get::<_, Option<String>>(4)?),
                "audience_definitions": json_col(r.get::<_, Option<String>>(5)?),
                "definition_card_id": r.get::<_, Option<String>>(6)?,
                "updated_at": r.get::<_, String>(7)?,
                "links": r.get::<_, i64>(8)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

// --------------------------------------------------------------- learning ---
//
// Doc 17 sections 2.1 and 2.2. Rows here are content: a prerequisite somebody
// drew and a reason somebody wrote. What the log decides about them is their
// status, which is why these writers carry an event and the projection is what
// moves it.

/// One prerequisite edge as it is drawn. Doc 17 section 2.1.
pub struct NewEdge<'a> {
    pub from_concept_id: &'a str,
    pub to_concept_id: &'a str,
    /// `prerequisite_of`, `part_of` or `contrasts_with`.
    pub relation: &'a str,
    pub proposed_by: &'a str,
    /// `proposed` for an agent's guess, `confirmed` for a shipped path or a
    /// person's own. Doc 01 section 4.10.
    pub status: &'a str,
    pub weight: f64,
}

/// Draw an edge, or return the one already there.
///
/// The pair plus the relation is unique, so proposing the same prerequisite
/// twice proposes it once, and a path loaded twice does not double its map.
pub fn propose_edge(store: &mut Store, e: NewEdge<'_>) -> Result<String> {
    if let Some(existing) = store
        .conn()
        .query_row(
            "SELECT id FROM concept_edge
             WHERE from_concept_id = ?1 AND to_concept_id = ?2 AND relation = ?3",
            params![e.from_concept_id, e.to_concept_id, e.relation],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(existing);
    }

    let id = new_id();
    let now = now_iso8601();
    let (row, from, to, relation, by, status, weight, at) = (
        id.clone(),
        e.from_concept_id.to_string(),
        e.to_concept_id.to_string(),
        e.relation.to_string(),
        e.proposed_by.to_string(),
        e.status.to_string(),
        e.weight,
        now,
    );

    store.append_with(
        NewEvent::new(
            "concept.edge_proposed.v1",
            json!({
                "edge_id": id,
                "from_concept_id": e.from_concept_id,
                "to_concept_id": e.to_concept_id,
                "relation": e.relation,
                // The status the edge was created with rides on the event, so a
                // replay does not demote a path's own edges to proposals.
                "status": e.status,
                "proposed_by": e.proposed_by,
            }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "INSERT INTO concept_edge (id, from_concept_id, to_concept_id, relation,
                     proposed_by, status, weight, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![row, from, to, relation, by, status, weight, at],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// Doc 01 section 4.10: the person confirms.
pub fn confirm_edge(store: &mut Store, edge_id: &str) -> Result<()> {
    store.append(NewEvent::new(
        "concept.edge_confirmed.v1",
        json!({ "edge_id": edge_id }),
        Provenance::user(),
    ))?;
    Ok(())
}

/// Doc 17 section 2.1: a rating is a claim, never evidence. The row moves
/// through the projection, so the claim is on the log before it is on the map.
pub fn rate_concept(store: &mut Store, concept_id: &str, rating: i64) -> Result<()> {
    store.append(NewEvent::new(
        "concept.rated.v1",
        json!({ "concept_id": concept_id, "rating": rating.clamp(0, 3) }),
        Provenance::user(),
    ))?;
    Ok(())
}

/// Doc 17 section 2.1's Mission: why the learner wants this.
pub fn create_mission(
    store: &mut Store,
    profile_id: &str,
    statement: &str,
    target_concept_ids: &[String],
    // Doc 17 section 5: the locators the path this mission came from was
    // written around, so a lesson reads them first. Empty for a mission the
    // learner wrote themselves, which is a mission with no path behind it.
    sources_hint: &[String],
) -> Result<String> {
    let id = new_id();
    let now = now_iso8601();
    let (row, profile, text, targets, hints, at) = (
        id.clone(),
        profile_id.to_string(),
        statement.trim().to_string(),
        serde_json::to_string(target_concept_ids)?,
        serde_json::to_string(sources_hint)?,
        now,
    );

    store.append_with(
        NewEvent::new(
            "mission.created.v1",
            json!({
                "mission_id": id,
                "statement": statement.trim(),
                "target_concept_ids": target_concept_ids,
                "sources_hint": sources_hint,
            }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "INSERT INTO mission (id, profile_id, statement, target_concept_ids,
                     sources_hint, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6)",
                params![row, profile, text, targets, hints, at],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// The mission a lesson is planned against, or none. Doc 17 section 2.1: "every
/// lesson is planned against an active mission so difficulty and examples fit
/// the reason". None is not an error: a learner who has not said why is planned
/// for by the map alone.
pub fn active_mission(store: &Store, profile_id: &str) -> Result<Value> {
    Ok(store
        .conn()
        .query_row(
            "SELECT id, statement, target_concept_ids, audience_id, sources_hint FROM mission
             WHERE profile_id = ?1 AND status = 'active' ORDER BY created_at DESC LIMIT 1",
            params![profile_id],
            |r| {
                Ok(json!({
                    "mission_id": r.get::<_, String>(0)?,
                    "statement": r.get::<_, String>(1)?,
                    "target_concept_ids": serde_json::from_str::<Value>(&r.get::<_, String>(2)?)
                        .unwrap_or_else(|_| json!([])),
                    "audience_id": r.get::<_, Option<String>>(3)?,
                    "sources_hint": serde_json::from_str::<Value>(&r.get::<_, String>(4)?)
                        .unwrap_or_else(|_| json!([])),
                }))
            },
        )
        .optional()?
        .unwrap_or(Value::Null))
}

/// The map as the Learning Planner and the Map view read it. Doc 17 section 6.
pub fn read_map(store: &Store, profile_id: &str) -> Result<(Vec<Value>, Vec<Value>)> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT c.id, c.term, c.learning_state, c.self_rating, c.mastery, c.difficulty_level,
                c.last_evidence_at, c.path_ids,
                (SELECT COUNT(*) FROM concept_link l
                 WHERE l.concept_id = c.id AND l.target_type = 'card') AS linked
         FROM concept c WHERE c.profile_id = ?1 ORDER BY c.term",
    )?;
    let concepts = stmt
        .query_map(params![profile_id], |r| {
            Ok(json!({
                "concept_id": r.get::<_, String>(0)?,
                "term": r.get::<_, String>(1)?,
                "learning_state": r.get::<_, Option<String>>(2)?,
                "self_rating": r.get::<_, Option<i64>>(3)?,
                "mastery": r.get::<_, Option<f64>>(4)?,
                "difficulty_level": r.get::<_, Option<i64>>(5)?,
                "last_evidence_at": r.get::<_, Option<String>>(6)?,
                "path_ids": r.get::<_, Option<String>>(7)?
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .unwrap_or_else(|| json!([])),
                "linked_cards": r.get::<_, i64>(8)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut stmt = conn.prepare(
        "SELECT e.id, e.from_concept_id, e.to_concept_id, e.relation, e.status, e.weight
         FROM concept_edge e
         JOIN concept c ON c.id = e.from_concept_id
         WHERE c.profile_id = ?1 ORDER BY e.created_at, e.id",
    )?;
    let edges = stmt
        .query_map(params![profile_id], |r| {
            Ok(json!({
                "edge_id": r.get::<_, String>(0)?,
                "from_concept_id": r.get::<_, String>(1)?,
                "to_concept_id": r.get::<_, String>(2)?,
                "relation": r.get::<_, String>(3)?,
                "status": r.get::<_, String>(4)?,
                "weight": r.get::<_, f64>(5)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok((concepts, edges))
}

/// What one concept on the map is linked to. Doc 17 section 6's node panel.
///
/// Cards and pages, both through `concept_link`, which is where a Librarian
/// proposal and a learner's confirmation both land. Rejected links are left
/// out: the learner said no, and a panel that still listed them would be
/// showing a decision it did not honour.
pub fn concept_links(store: &Store, concept_id: &str) -> Result<(Vec<Value>, Vec<Value>)> {
    let conn = store.conn();
    let mut stmt = conn.prepare(
        "SELECT c.id, c.board_id, c.question, b.title
         FROM concept_link l
         JOIN card c ON c.id = l.target_ref
         JOIN board b ON b.id = c.board_id
         WHERE l.concept_id = ?1 AND l.target_type = 'card' AND l.status != 'rejected'
         ORDER BY c.created_at",
    )?;
    let cards = stmt
        .query_map(params![concept_id], |r| {
            Ok(json!({
                "card_id": r.get::<_, String>(0)?,
                "board_id": r.get::<_, String>(1)?,
                "question": r.get::<_, String>(2)?,
                "board_title": r.get::<_, String>(3)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Doc 16 section 3.1's wikilink to a concept, read from the other end: the
    // pages that name this concept are the pages a learner wrote about it.
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.id, p.title
         FROM page_link l
         JOIN page p ON p.id = l.from_page_id
         WHERE l.target_kind = 'concept' AND l.target_id = ?1
         ORDER BY p.title",
    )?;
    let pages = stmt
        .query_map(params![concept_id], |r| {
            Ok(json!({
                "page_id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok((cards, pages))
}

/// A concept by term, created if this profile has none. Doc 17 section 2.1:
/// loading a path "creates or links the concepts".
pub fn ensure_concept(
    store: &mut Store,
    profile_id: &str,
    doctrine_pack_id: &str,
    term: &str,
) -> Result<String> {
    if let Some(existing) = concept_by_term_or_alias(store, profile_id, term)? {
        return Ok(existing);
    }
    let id = new_id();
    let now = now_iso8601();
    let (row, profile, text, pack, at) = (
        id.clone(),
        profile_id.to_string(),
        term.trim().to_string(),
        doctrine_pack_id.to_string(),
        now,
    );
    store.append_with(
        NewEvent::new(
            "concept.proposed.v1",
            json!({ "concept_id": id, "term": term.trim(), "proposed_by": "path" }),
            Provenance::user(),
        ),
        move |tx| {
            tx.execute(
                "INSERT INTO concept (id, profile_id, term, doctrine_pack_id, status,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'confirmed', ?5, ?5)",
                params![row, profile, text, pack, at],
            )?;
            Ok(())
        },
    )?;
    Ok(id)
}

/// The concept this title names, by term or by alias, case insensitively.
///
/// Doc 16 section 3.1: "a wikilink whose title matches a Concept term or alias
/// links to the concept". The alias match is a scan over the profile's
/// concepts, which is bounded by the glossary rather than by the vault: a
/// profile has tens of concepts and thousands of links into them.
pub fn concept_by_term_or_alias(store: &Store, profile_id: &str, title: &str) -> Result<Option<String>> {
    let conn = store.conn();
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM concept WHERE profile_id = ?1 AND term = ?2 COLLATE NOCASE LIMIT 1",
            params![profile_id, title.trim()],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(Some(id));
    }

    let wanted = title.trim().to_lowercase();
    let mut stmt = conn.prepare("SELECT id, aliases FROM concept WHERE profile_id = ?1")?;
    let rows = stmt.query_map(params![profile_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    for row in rows {
        let (id, aliases) = row?;
        let Some(aliases) = aliases else { continue };
        let Ok(list) = serde_json::from_str::<Value>(&aliases) else {
            continue;
        };
        let matched = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|a| a.trim().to_lowercase() == wanted);
        if matched {
            return Ok(Some(id));
        }
    }
    Ok(None)
}
