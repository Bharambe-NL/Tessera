# 01. Data Model v0.2

Changelog v0.2: added `Card.builds_on`, source class `own_card`, ConceptLink relation `builds_on`, `Profile.memory_enabled`, `Board.mode`, LearnSession (see 14), and the event names listed in 13. See 15 for the memory design.

Product name: Tessera (confirmed by the owner 2026-08-30; the working name was Canvas). Register: working. Status: draft for review, first document of the Mode B spec set.

## 1. What this document decides

This is the substrate every agent reads from and writes to. It fixes the entities, their fields, their relationships, the event log that records everything the agents do, and the bundle format that lets a board travel between people. Later documents (agent specs, UX, architecture) depend on the decisions here and must not redefine them.

Scoping decisions this document honours, from Phase 1:

- Local desktop application, single author per board, models reached through the user's own keys.
- Full automation with an audit trail. The Verifier flags; the user reviews flags only.
- Substrate is domain neutral. Finance is the first doctrine pack. A second vertical adds a pack, never a schema.
- Sources and Concepts are first class and shared across boards.
- Boards travel as portable bundles. Live multi-user editing is out of v1.
- Synthetic first for evaluation.

Two conventions run through the whole model.

1. Every entity that an agent produces carries `schema_version`, `produced_by` (agent id and model alias), and `run_id`. This is Pattern 7 (structured output enforcement) and Pattern 13 (provenance labelling) applied at the storage layer rather than only at the message layer.
2. Nothing an agent produced is ever edited in place. A revision creates a new row with `supersedes` pointing at the old one. The event log records the transition. This is what makes the audit trail complete without a separate audit table.

## 2. Substrate and doctrine sort

The skill requires this sort before any entity is specified. The table is the sort.

| Component | Layer | Rationale |
|---|---|---|
| Board, Card, Visual, Ink, Note, Image | Substrate | Same shape for a bank and a biotech. |
| Source, Passage, Citation | Substrate | Provenance mechanics do not change by domain. |
| Concept, ConceptLink | Substrate | The graph is generic; the content is doctrine. |
| Run, Step, Event | Substrate | Pattern 24 event-sourced run state. |
| Flag, Review | Substrate | Flag mechanics are generic; flag rules are doctrine. |
| Exercise, Attempt | Substrate | Generation and scoring are generic. |
| Profile, ModelKey, ModelPolicy | Substrate | Provider abstraction, Pattern 21. |
| Retriever registration, index config | Substrate | The registry is generic. |
| Source hierarchy (which sources outrank which) | Doctrine | Finance: regulator over filing over press. |
| Audience definitions and their vocabularies | Doctrine | "Risk" and "Engineering" are this team's audiences. |
| Flag rules (what the Verifier must hold back) | Doctrine | Advice language, unsourced numbers, stale regulation. |
| Retriever set and folder inclusion defaults | Doctrine | Which corpora and which folders. |
| Freshness thresholds per source class | Doctrine | Regulation ages differently from a product doc. |
| Exercise templates | Doctrine | What "understanding" looks like per domain. |

Doctrine is stored as data in a `DoctrinePack` entity (section 4.13), versioned and citable. It is never hard coded in an agent prompt.

## 3. Entity inventory

Seventeen source of truth entities and four operational tables.

Source of truth:

1. Board
2. Card
3. Visual
4. Ink
5. Note
6. Image
7. Source
8. Passage
9. Citation
10. Concept
11. ConceptLink
12. Flag
13. Review
14. Exercise
15. Attempt
16. Profile
17. DoctrinePack

Operational:

18. Run
19. Step
20. Event
21. IndexEntry

Identifiers are ULIDs (time sortable, safe to merge across machines when bundles are imported). Timestamps are ISO 8601 with offset. All text is UTF-8. Money and numbers inside content are strings with an explicit unit field where the schema calls for it; the model never stores a number it computed.

## 4. Entities

Field tables use: `req` (required), `opt` (optional), `sys` (set by the harness, never by an agent or the user).

### 4.1 Board

A board is one canvas: a set of cards, authored material, and a viewport. It belongs to one profile.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| profile_id | ulid | sys | Owner. Single author in v1. |
| title | string | req | User named or taken from the first question. |
| named_by_user | bool | sys | Prevents the first question from overwriting a user title. |
| doctrine_pack_id | ulid | req | The pack whose rules apply on this board. |
| context | text | opt | Seed context when spun off from a block on another board. |
| seed_label | string | opt | The block label that created this board. |
| parent_board_id | ulid | opt | The board it was spun off from. |
| forked_from_bundle_id | string | opt | Set when imported from someone else's bundle. Section 7. |
| viewport | json {x, y, k} | sys | Last camera position. |
| default_depth | enum fast, deep, research | req | Per board default. Cards inherit. |
| mode | enum explore, learn | req | See 14. |
| default_model_policy_id | ulid | opt | Overrides profile policy. |
| status | enum active, trashed | sys | |
| trashed_at | timestamp | opt | Purge after 30 days. Purge is an event, never a silent delete. |
| created_at, updated_at | timestamp | sys | |

### 4.2 Card

A card is one question and one answer on a board. Cards form a tree through `parent_card_id` and `kind`.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| board_id | ulid | sys | |
| parent_card_id | ulid | opt | Null for a root. |
| kind | enum root, follow, branch, read, exercise | req | `read` is produced from an Image or Ink by the Reader. |
| anchor_text | string | opt | The highlighted phrase for a branch, or the block label for a block spawned branch. |
| anchor_block_ref | string | opt | Path into the parent Visual (section 4.3) when spawned from a block. |
| question | text | req | What the user typed, or what the harness composed for a branch. |
| depth | enum fast, deep, research | req | Inherited from board at creation, overridable. |
| audience_id | string | opt | Doctrine audience code. Null means the author's own register. |
| answer | text | opt | Prose. Citation markers are `[n]` where n is a Citation.ordinal. |
| findings | json array of {text, citation_ordinals[]} | opt | Deep and research only. |
| visual_id | ulid | opt | The current Visual. |
| status | enum queued, running, done, flagged, failed | sys | `flagged` means at least one open Flag exists. |
| run_id | ulid | sys | The run that produced the current answer. |
| supersedes | ulid | opt | Previous Card version when rerun with another model or depth. |
| produced_by | json {agent_id, model_alias, provider} | sys | |
| schema_version | string | sys | Card schema, currently "1.0". |
| confidence | float 0 to 1 | sys | From the Verifier, section 4.12. |
| builds_on | json array of {board_id, card_id, verified_at} | sys | Prior verified cards recalled by the boards retriever as context. Never evidence. |
| position | json {x, y, dx, dy, pinned} | sys | Layout slot plus user offset. |
| created_at, updated_at | timestamp | sys | |

A rerun creates a new Card with `supersedes` set; the board shows the latest and the header offers the chain. The old Card keeps its Visual, Citations, and Flags.

### 4.3 Visual

The structured payload the frontend renders. Stored separately from Card so a Visual can be revised without a Card rerun (for instance, the Verifier removes a node) and so the Exercise agent can read it without loading prose.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| card_id | ulid | sys | |
| type | enum tree, table, list, steps, figure, image, chart, widget | req | `chart` and `widget` reserved for v1.1; schema stubs in section 9. |
| title | string | req | |
| payload | json | req | Type specific, section 4.3.1. |
| block_index | json array of {ref, label, citation_ordinals[]} | sys | Every clickable block, its path, and which citations support it. Built by the Visualizer, checked by the Verifier. |
| supersedes | ulid | opt | |
| produced_by | json | sys | |
| schema_version | string | sys | "1.0" |

#### 4.3.1 Payload schemas

Tree: `{root: Node}` where `Node = {label, note, citation_ordinals[], children: Node[]}`. Depth at most 3 below root. At most 6 children per node.

Table: `{columns: string[], rows: string[][], bottom_line: {head, text, citation_ordinals[]}}`. Every cell may carry a citation marker inline.

List: `{groups: [{heading, items: [{name, detail, citation_ordinals[]}]}], bottom_line}`.

Steps: `{steps: [{label, note, citation_ordinals[]}]}`.

Figure: `{svg: string, caption}`. The svg is sanitised by the harness before storage (no script, no foreignObject, no external references). Sanitisation is a Step with its own event.

Image: `{image_id, caption, prompt}`. Generated images reference an Image row (section 4.6) so they get the same treatment as pasted ones.

The `block_index` `ref` is a JSON pointer into `payload` (for example `/root/children/2` or `/rows/3/1`). The frontend uses it to raise "Investigate this further" with an exact reference, and the Verifier uses it to bind blocks to citations.

### 4.4 Ink

One stroke.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| board_id | ulid | sys | |
| colour | string | req | Token name, never a raw colour value, so a bundle renders in the recipient's theme. |
| width | float | req | |
| points | json array of [x, y] | req | Board coordinates, rounded to integers. |
| created_at | timestamp | sys | |

Ink is authored material. It has no `produced_by`.

### 4.5 Note

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| board_id | ulid | sys | |
| text | text | req | |
| colour | string | req | Token name. |
| position | json {x, y, w, h} | sys | |
| created_at, updated_at | timestamp | sys | |

### 4.6 Image

Both pasted images and generated ones.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| board_id | ulid | sys | |
| origin | enum pasted, generated, sketch_raster | req | `sketch_raster` is the flattened Ink plus Notes sent to the Reader. |
| blob_ref | string | req | Content addressed path in the blob store, sha256 of bytes. |
| mime | string | req | |
| width, height | int | req | |
| position | json {x, y} | sys | |
| generation | json {prompt, model_alias, provider, run_id} | opt | Generated only. |
| source_ink_ids, source_note_ids | ulid[] | opt | Sketch raster only. |
| created_at | timestamp | sys | |

Images are stored once by hash. A bundle carries them by hash so a forked board never duplicates bytes.

### 4.7 Source

A source is a thing that can be cited. It is shared across all boards owned by the profile and it survives board deletion.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| class | enum web, regulatory, local_document, structured_query, user_supplied, own_card | req | Doctrine hierarchy ranks these. `own_card` may never be the sole support for a numeric or regulatory claim. |
| title | string | req | |
| locator | string | req | URL, file path, corpus id plus document id, or query text. |
| site_or_issuer | string | opt | "eba.europa.eu", "DNB", or a folder name. |
| published_at | timestamp | opt | When the source itself was published. |
| retrieved_at | timestamp | sys | First retrieval. |
| last_verified_at | timestamp | sys | Last time the retriever confirmed the locator still resolves and the content hash matches. |
| content_hash | string | sys | sha256 of the retrieved content at `retrieved_at`. |
| freshness_class | string | req | Doctrine code, drives the freshness gate (Pattern 5). |
| trust_rank | int | sys | Assigned from the doctrine source hierarchy at retrieval. Lower is more trusted. |
| dedupe_key | string | sys | Normalised locator. Two retrievals of the same page yield one Source. |
| created_at | timestamp | sys | |

### 4.8 Passage

The exact span of a source that a retriever returned. A source can have many passages. Citations point at passages, never at whole sources, so the audit trail shows the words the model saw.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| source_id | ulid | sys | |
| text | text | req | Verbatim as retrieved. Stored for audit; never displayed in full inside a card. |
| location | json | opt | Page, section, character offsets, or row range. |
| retrieved_in_run | ulid | sys | |
| retrieved_by | string | sys | Retriever id. |
| embedding_ref | string | opt | Pointer into IndexEntry when indexed locally. |

### 4.9 Citation

The binding between a claim in a card and a passage.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| card_id | ulid | sys | |
| ordinal | int | sys | The `[n]` shown in the answer. Unique per card. |
| passage_id | ulid | req | |
| claim_span | json {start, end} | req | Character offsets into `Card.answer`, or a `block_ref` when the claim lives in a Visual. |
| binding | enum answer, finding, block | req | Where the marker sits. |
| verifier_verdict | enum supported, weak, unsupported, unchecked | sys | Set by the Verifier. |
| supersedes | ulid | opt | |

The Verifier's core check reads: for every claim span, at least one Citation with verdict `supported`; for every Visual block, at least one Citation or an explicit `no_claim` marking in the block index. In fast mode citations are not required and the verdict stays `unchecked`; the card header shows this.

### 4.10 Concept

A term with a shared meaning across boards. The unit of "shared understanding".

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| profile_id | ulid | sys | |
| term | string | req | Canonical spelling. |
| aliases | string[] | opt | |
| definition | text | opt | Short, in the author's words or adopted from a card with attribution. |
| definition_card_id | ulid | opt | The card the definition was adopted from. |
| audience_definitions | json map audience_id to text | opt | The same term explained per audience. Doctrine supplies the audience list. |
| doctrine_pack_id | ulid | req | Terms belong to a pack. |
| status | enum proposed, confirmed | sys | Agents propose; the user confirms. |
| supersedes | ulid | opt | |
| created_at, updated_at | timestamp | sys | |

### 4.11 ConceptLink

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| concept_id | ulid | sys | |
| target_type | enum card, visual_block, source, concept | req | |
| target_ref | string | req | Entity id, or entity id plus block ref. |
| relation | enum explains, mentions, defines, contradicts, related_to, builds_on | req | |
| proposed_by | json {agent_id or user} | sys | |
| status | enum proposed, confirmed, rejected | sys | |

A concept is a node; links are how boards touch it. Two boards that both cite the same Concept share it, which is the mechanism behind "when two boards touch PSD3 they touch the same node".

### 4.12 Flag

What the Verifier holds back. Full automation means the user only sees these.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| card_id | ulid | sys | |
| rule_id | string | req | Doctrine rule code that fired. |
| severity | enum info, warn, block | req | `block` hides the affected content until reviewed. |
| target | json {kind: answer_span or block or citation or whole_card, ref} | req | |
| reason | text | req | Plain sentence for the reviewer. |
| evidence | json | opt | The passage, the number, the stale date. Pattern 14 bundle when the cause is unknown. |
| status | enum open, accepted, dismissed, fixed | sys | |
| review_id | ulid | opt | |
| created_at | timestamp | sys | |

### 4.13 Review

A user decision on a Flag or a set of Flags.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| flag_ids | ulid[] | req | |
| decision | enum accept, dismiss, rerun, edit | req | |
| note | text | opt | |
| resulting_card_id | ulid | opt | When the decision reran or edited the card. |
| decided_at | timestamp | sys | |

Reviews are immutable. Changing your mind creates another Review.

### 4.14 Exercise

Generated from a board or a card to check understanding.

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| board_id | ulid | sys | |
| scope | json {card_ids[] or whole_board} | req | |
| template_id | string | req | Doctrine template code. |
| audience_id | string | opt | |
| items | json array of {id, prompt, options[], answer_id, explanation, source_card_id, citation_ordinals[]} | req | Every item points at the card it was made from. |
| produced_by | json | sys | |
| schema_version | string | sys | |

### 4.15 Attempt

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| exercise_id | ulid | sys | |
| answers | json map item id to option id | req | |
| score | json {correct, total} | sys | |
| taken_at | timestamp | sys | |

Attempts stay local to the profile and are excluded from bundles by default.

### 4.16 Profile

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| name, role | string | opt | |
| context | text | opt | Injected into every model call. |
| standing_instructions | text | opt | Injected into every model call. |
| default_depth | enum | req | |
| memory_enabled | bool | req | Boards retriever on by default. |
| default_doctrine_pack_id | ulid | req | |
| model_policy | json | req | Section 5. |
| retriever_config | json | req | Folder inclusions, corpus subscriptions, search key ref. Keys themselves are never in this row. |
| created_at, updated_at | timestamp | sys | |

Model keys live in the OS keychain (macOS Keychain, Windows Credential Manager). The Profile stores a `ModelKey` list of `{key_ref, provider, label, active}` where `key_ref` names the keychain entry. The database never holds a secret.

### 4.17 DoctrinePack

| Field | Type | Req | Notes |
|---|---|---|---|
| id | ulid | sys | |
| code | string | req | "finance-eu", "general". |
| version | string | req | Semver. Boards pin a version. |
| audiences | json array of {id, name, vocabulary_notes} | req | |
| source_hierarchy | json array of {class, issuer_pattern, trust_rank} | req | |
| freshness_classes | json map code to {max_age_days, on_stale: flag or rerun} | req | |
| flag_rules | json array of {rule_id, severity, description, detector} | req | `detector` names a deterministic check or a model prompt id. |
| retrievers | json array of retriever ids with default config | req | |
| exercise_templates | json array | req | |
| rulings | json array of {id, text, cites[], adopted_at} | opt | Accumulated decisions, written by the occupants over time. Pattern 16 team memory, scoped to a pack. |
| created_at | timestamp | sys | |

Doctrine packs are files (JSON) checked into the app bundle for the built in packs and importable for custom ones. The database row caches the active version.

## 5. Model policy

Router plus user override, from Phase 1.

```
model_policy: {
  version: "1.0",
  stages: {
    route:      { alias: "small",    fallback: ["medium"] },
    plan:       { alias: "medium",   fallback: ["frontier"] },
    retrieve:   { alias: null },                       // no model
    synthesize: { alias: "frontier", fallback: ["medium"] },
    visualize:  { alias: "frontier", fallback: ["medium"] },
    read:       { alias: "vision",   fallback: [] },
    verify:     { alias: "medium",   fallback: ["frontier"] },
    exercise:   { alias: "medium",   fallback: [] }
  },
  aliases: {
    small:    { provider: "anthropic", model: "claude-haiku-4-5",  key_ref: "anthropic-team" },
    medium:   { provider: "anthropic", model: "claude-sonnet-4-6", key_ref: "anthropic-team" },
    frontier: { provider: "anthropic", model: "claude-opus-4-8",   key_ref: "anthropic-team" },
    vision:   { provider: "google",    model: "gemini-...",         key_ref: "google-personal" }
  }
}
```

Aliases decouple the stage from the provider (Pattern 21). A card override replaces the alias for one stage on one run and is recorded in `Card.produced_by`. Model names above are placeholders for the architecture spec to fix.

## 6. Runs, steps, events

Event sourced run state (Pattern 24). The Event table is the audit trail; Run and Step are projections kept for query speed and rebuilt from events on demand.

### 6.1 Run

| Field | Type | Notes |
|---|---|---|
| id | ulid | |
| board_id, card_id | ulid | The card being produced. |
| kind | enum card, read, exercise, index, verify_only | |
| depth | enum | |
| model_policy_snapshot | json | The resolved aliases at run start. |
| doctrine_pack_version | string | |
| status | enum running, done, failed, cancelled | |
| started_at, ended_at | timestamp | |
| cost | json {input_tokens, output_tokens, calls, by_provider} | |

### 6.2 Step

| Field | Type | Notes |
|---|---|---|
| id | ulid | |
| run_id | ulid | |
| agent_id | string | router, planner, retriever.web, retriever.local, retriever.regulatory, synthesizer, visualizer, reader, verifier, exercise, harness |
| sequence | int | Monotonic within the run. |
| task_packet | json | Pattern 2. Stored verbatim. |
| output | json | Validated against the agent's schema before storage (Pattern 7). |
| model_call | json {provider, model, prompt_hash, input_tokens, output_tokens, latency_ms} | Null for retrievers and harness steps. |
| status | enum done, retried, failed | |
| failure | json {type, detail, recovery} | Pattern 4 taxonomy. |
| started_at, ended_at | timestamp | |

Prompts are stored by hash with the full text in the blob store, so the audit trail can reproduce any call and the main database stays small.

### 6.3 Event

```
{
  event_id: ulid,
  event_type: string,            // versioned, e.g. "card.answered.v1"
  payload: json,
  provenance: {
    source: live | test | replay | healthcheck | harness,
    emitter_id: string,
    emitter_type: agent | harness | user | retriever,
    run_id: ulid | null,
    trust_level: verified | unverified | degraded
  },
  sequence: { monotonic_index: int, causal_parent_id: ulid | null },
  timestamp
}
```

The v1 event vocabulary:

```
board.created.v1        board.renamed.v1       board.trashed.v1      board.restored.v1
board.purged.v1         board.exported.v1      board.imported.v1
card.requested.v1       card.routed.v1         card.planned.v1
retrieval.started.v1    retrieval.completed.v1 { retriever, source_ids, passage_ids }
source.created.v1       source.deduplicated.v1 source.stale.v1
card.synthesized.v1     visual.produced.v1     visual.sanitised.v1
citation.bound.v1       verify.completed.v1 { verdicts, flags }
flag.raised.v1          review.decided.v1
card.answered.v1        card.rerun.v1          card.failed.v1 { failure }
card.superseded.v1
concept.proposed.v1     concept.confirmed.v1   concept.linked.v1
image.pasted.v1         image.generated.v1     sketch.rasterised.v1  read.completed.v1
exercise.generated.v1   attempt.recorded.v1
ink.added.v1            ink.erased.v1          note.added.v1  note.edited.v1
index.folder_added.v1   index.updated.v1       index.folder_removed.v1
model.call.v1           model.fallback.v1      schema.violation.v1
```

User actions are events too (`emitter_type: user`). That is what makes "who changed what" answerable without a second mechanism.

Replay: a run can be replayed from its events with `provenance.source: replay`. Policy checks do not fire on replay. This is the debugging path for a bad card.

## 7. The portable bundle

The sharing mechanism for v1. A bundle is a zip with a manifest.

```
board.bundle/
  manifest.json          { bundle_id, format_version: "1.0", exported_at, exported_by (name only),
                           board_id, doctrine_pack: {code, version}, includes: {...} }
  board.json             Board row
  cards.jsonl            all Card versions (supersedes chain intact)
  visuals.jsonl
  citations.jsonl
  flags.jsonl            open and resolved
  reviews.jsonl
  ink.jsonl  notes.jsonl images.jsonl
  sources.jsonl          only sources cited on this board
  passages.jsonl         only passages cited on this board
  concepts.jsonl         concepts linked from this board, with links
  exercises.jsonl        optional
  events.jsonl           the board's events, optional but on by default
  blobs/<sha256>         images, sanitised svgs, prompt texts referenced by steps
```

Rules:

- Import never overwrites. Imported rows keep their ids; the importing profile records `forked_from_bundle_id`. Because ids are ULIDs, collisions do not occur.
- Sources and Concepts merge by `dedupe_key` and by `term` respectively. On a term collision the importer keeps both, marks the incoming one `proposed`, and links them with `related_to` for the user to reconcile.
- Passages are included so the recipient can audit citations without re-retrieving. Local document passages carry the text but never the file path beyond the file name, and the exporter shows a checklist of local document sources before export so nothing leaves by accident.
- Attempts, profile context, model keys, and folder paths are never included.
- Events are included so the recipient can see how the board was built. An "export without history" option drops `events.jsonl` and all `Step.task_packet` content.

Fork: opening a bundle creates a new Board with `forked_from_bundle_id`. Follow-ups on a forked board start new runs; the original author's runs remain readable and marked as imported.

## 8. Storage mapping

SQLite, one file per profile, WAL mode. Blob store is a directory of content addressed files beside it. Local vector index (sqlite-vec or equivalent, decided in the architecture spec) holds `IndexEntry` rows:

| Field | Notes |
|---|---|
| id | |
| passage_id or document_chunk_ref | Local documents are chunked at index time; a chunk becomes a Passage only when cited. |
| embedding | vector |
| folder_id | Which watched folder. Removing the folder removes its entries and emits `index.folder_removed.v1`. |
| content_hash | Reindex when the file changes. |

Retention: events and steps are kept indefinitely by default. A profile setting can compact steps older than N days into a summary event (Pattern 24: compaction operates on the view, the log stays).

## 9. Reserved for v1.1

Two Visual types have schema stubs now so the block index and citation binding do not need redesign later.

Chart: `{kind: bar | line | area, series: [{name, points: [{x, y, citation_ordinals[]}]}], unit, source_query_step_id}`. Every point cites a passage or a structured query step. The Visualizer may never invent a point.

Widget: `{html: string, interactions: [{id, kind: toggle | slider | region, binds_to: block_ref}], sandbox_policy: string}`. Rendered in a sandboxed iframe with a message bridge. The Verifier checks the html against an allowlist before storage. Interactions raise the same "investigate" affordance as static blocks by emitting the bound `block_ref`.

Also reserved: a `Share` entity for v2 sync, with `board_id`, `recipient`, `permission`, so v1 bundles can be migrated into shares without changing Board.

## 10. Relationships, summarised

```
Profile 1..* Board 1..* Card 0..1 Visual
Card 0..* Citation *..1 Passage *..1 Source
Card 0..* Flag 0..1 Review
Board 0..* Ink, Note, Image
Image 0..1 Card (kind read)
Concept 0..* ConceptLink -> Card | Visual block | Source | Concept
Board 0..* Exercise 0..* Attempt
Board *..1 DoctrinePack (pinned version)
Run 1..* Step; Run 1..* Event; Card *..1 Run
```

Cardinalities that constrain agents: a Card has exactly one current Visual (or none in fast mode when the Visualizer declines); a Citation binds exactly one passage; a Flag targets exactly one card; a Source has at least one Passage once cited.

## 11. Open questions for review

1. Should `Card.answer` store the citation markers inline, or should markers be derived from `Citation.claim_span` at render time? Inline is simpler and matches the prototype; derived survives edits to the answer text. Proposal: derived, with inline as the export rendering.
2. Passage text for local documents: store verbatim, or store a hash plus offsets and re-read the file on demand? Verbatim makes bundles complete; hash plus offsets keeps sensitive text out of the database. Proposal: verbatim by default, with a doctrine flag rule for folders marked sensitive that stores offsets only and blocks export.
3. Do Concepts belong to a profile or to a doctrine pack? The model above says both (profile owns the row, pack scopes the term). If a team standardises on a pack, the pack's `rulings` may be the better home for confirmed definitions. Decide with the first Risk plus Product test.
4. Regulatory corpus versioning: a Source for "CRR3 Article 92" should point at a specific consolidated version. Whether the version is part of `locator` or a separate field affects how staleness is detected. Proposal: separate `version_ref` field on Source, populated by the regulatory retriever only.

Next document: 02, Synthetic data generator, which produces boards, sources, and passages with known ground truth so the Verifier and the citation binding can be evaluated before any real corpus is indexed.
