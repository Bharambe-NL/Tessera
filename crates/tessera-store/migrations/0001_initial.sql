-- Tessera schema v1. Doc 01 sections 4, 6 and 8.
--
-- Two conventions run through the whole model (doc 01 section 1):
--   1. Every entity an agent produced carries schema_version, produced_by and
--      run_id.
--   2. Nothing an agent produced is ever edited in place. A revision inserts a
--      new row with `supersedes` pointing at the old one.
--
-- Identifiers are ULIDs stored as TEXT: time sortable, and safe to merge across
-- machines when a bundle is imported. Timestamps are ISO 8601 with offset,
-- stored as TEXT so they survive a round trip through a bundle unchanged.
--
-- JSON columns hold documents validated by tessera-schema before insert. SQLite
-- does not enforce them; the schema guard does.

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------- profile ---

CREATE TABLE profile (
  id                      TEXT PRIMARY KEY,
  name                    TEXT,
  role                    TEXT,
  context                 TEXT,
  standing_instructions   TEXT,
  default_depth           TEXT NOT NULL CHECK (default_depth IN ('fast', 'deep', 'research')),
  default_doctrine_pack_id TEXT NOT NULL,
  -- Doc 01 section 5. Contains aliases and per stage resolution, never a secret.
  model_policy            TEXT NOT NULL,
  -- Folder inclusions, corpus subscriptions, key references. Never a key.
  retriever_config        TEXT NOT NULL,
  created_at              TEXT NOT NULL,
  updated_at              TEXT NOT NULL
) STRICT;

-- Doc 01 section 4.16: the database never holds a secret. key_ref names an entry
-- in the OS keychain, which is the only place the value exists.
CREATE TABLE model_key (
  key_ref    TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  provider   TEXT NOT NULL,
  label      TEXT NOT NULL,
  active     INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
  created_at TEXT NOT NULL
) STRICT;

-- --------------------------------------------------------------- doctrine ---

CREATE TABLE doctrine_pack (
  id                 TEXT PRIMARY KEY,
  code               TEXT NOT NULL,
  version            TEXT NOT NULL,
  audiences          TEXT NOT NULL,
  source_hierarchy   TEXT NOT NULL,
  freshness_classes  TEXT NOT NULL,
  flag_rules         TEXT NOT NULL,
  retrievers         TEXT NOT NULL,
  exercise_templates TEXT NOT NULL,
  rulings            TEXT,
  created_at         TEXT NOT NULL,
  UNIQUE (code, version)
) STRICT;

-- ------------------------------------------------------------------ board ---

CREATE TABLE board (
  id                      TEXT PRIMARY KEY,
  profile_id              TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  title                   TEXT NOT NULL,
  named_by_user           INTEGER NOT NULL DEFAULT 0 CHECK (named_by_user IN (0, 1)),
  doctrine_pack_id        TEXT NOT NULL REFERENCES doctrine_pack(id),
  context                 TEXT,
  seed_label              TEXT,
  parent_board_id         TEXT REFERENCES board(id),
  forked_from_bundle_id   TEXT,
  viewport                TEXT NOT NULL DEFAULT '{"x":0,"y":0,"k":1}',
  default_depth           TEXT NOT NULL CHECK (default_depth IN ('fast', 'deep', 'research')),
  default_model_policy_id TEXT,
  -- Doc 14 section 2.
  mode                    TEXT NOT NULL DEFAULT 'explore' CHECK (mode IN ('explore', 'learn')),
  status                  TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'trashed')),
  trashed_at              TEXT,
  created_at              TEXT NOT NULL,
  updated_at              TEXT NOT NULL
) STRICT;

CREATE INDEX board_profile_status ON board(profile_id, status, updated_at DESC);

-- ------------------------------------------------------------------- runs ---
-- Run and Step are projections of the event log, kept for query speed and
-- rebuildable from events on demand. Doc 01 section 6.

CREATE TABLE run (
  id                    TEXT PRIMARY KEY,
  board_id              TEXT REFERENCES board(id) ON DELETE CASCADE,
  card_id               TEXT,
  kind                  TEXT NOT NULL CHECK (kind IN ('card', 'read', 'exercise', 'index', 'verify_only')),
  depth                 TEXT CHECK (depth IN ('fast', 'deep', 'research')),
  model_policy_snapshot TEXT NOT NULL,
  doctrine_pack_version TEXT NOT NULL,
  status                TEXT NOT NULL CHECK (status IN ('running', 'done', 'failed', 'cancelled')),
  started_at            TEXT NOT NULL,
  ended_at              TEXT,
  cost                  TEXT NOT NULL DEFAULT '{"input_tokens":0,"output_tokens":0,"calls":0,"by_provider":{}}',

  -- Work ledger, doc 10 section 6. A crash mid run leaves a claim that the next
  -- start reclaims or marks failed.
  claimed_by            TEXT,
  claimed_at            TEXT,
  heartbeat_at          TEXT
) STRICT;

CREATE INDEX run_board ON run(board_id, started_at DESC);
CREATE INDEX run_liveness ON run(status, heartbeat_at) WHERE status = 'running';

CREATE TABLE step (
  id          TEXT PRIMARY KEY,
  run_id      TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
  agent_id    TEXT NOT NULL,
  sequence    INTEGER NOT NULL,
  task_packet TEXT NOT NULL,
  output      TEXT,
  -- Null for retriever and harness steps. prompt_hash points into the blob store,
  -- so the audit trail can reproduce a call without bloating the database.
  model_call  TEXT,
  status      TEXT NOT NULL CHECK (status IN ('done', 'retried', 'failed')),
  failure     TEXT,
  started_at  TEXT NOT NULL,
  ended_at    TEXT,
  UNIQUE (run_id, sequence)
) STRICT;

CREATE INDEX step_run ON step(run_id, sequence);

-- ------------------------------------------------------------------ event ---
-- The audit trail. Append only: there is no UPDATE or DELETE path in the store,
-- and compaction summarises Steps into an event, never the event log itself.
-- Doc 01 section 6.3.

CREATE TABLE event (
  event_id         TEXT PRIMARY KEY,
  monotonic_index  INTEGER NOT NULL UNIQUE,
  event_type       TEXT NOT NULL,
  payload          TEXT NOT NULL,

  -- Provenance envelope, doc 01 section 6.3. Flattened into columns because
  -- every read filters on source and run_id.
  source           TEXT NOT NULL CHECK (source IN ('live', 'test', 'replay', 'healthcheck', 'harness')),
  emitter_id       TEXT NOT NULL,
  emitter_type     TEXT NOT NULL CHECK (emitter_type IN ('agent', 'harness', 'user', 'retriever')),
  run_id           TEXT,
  trust_level      TEXT NOT NULL CHECK (trust_level IN ('verified', 'unverified', 'degraded')),

  causal_parent_id TEXT,
  board_id         TEXT,
  card_id          TEXT,
  timestamp        TEXT NOT NULL
) STRICT;

CREATE INDEX event_board ON event(board_id, monotonic_index);
CREATE INDEX event_card ON event(card_id, monotonic_index);
CREATE INDEX event_run ON event(run_id, monotonic_index);
CREATE INDEX event_type_idx ON event(event_type, monotonic_index);

-- The append only guarantee, enforced by the database rather than by discipline.
CREATE TRIGGER event_no_update BEFORE UPDATE ON event BEGIN
  SELECT RAISE(ABORT, 'the event log is append only');
END;

CREATE TRIGGER event_no_delete BEFORE DELETE ON event BEGIN
  SELECT RAISE(ABORT, 'the event log is append only');
END;

-- Hands out monotonic_index under the same transaction as the insert, so two
-- writers cannot claim the same index.
CREATE TABLE event_sequence (
  id   INTEGER PRIMARY KEY CHECK (id = 1),
  next INTEGER NOT NULL
) STRICT;

INSERT INTO event_sequence (id, next) VALUES (1, 1);

-- ------------------------------------------------------------------- card ---

CREATE TABLE card (
  id               TEXT PRIMARY KEY,
  board_id         TEXT NOT NULL REFERENCES board(id) ON DELETE CASCADE,
  parent_card_id   TEXT REFERENCES card(id),
  kind             TEXT NOT NULL CHECK (kind IN ('root', 'follow', 'branch', 'read', 'exercise')),
  anchor_text      TEXT,
  anchor_block_ref TEXT,
  question         TEXT NOT NULL,
  depth            TEXT NOT NULL CHECK (depth IN ('fast', 'deep', 'research')),
  audience_id      TEXT,
  answer           TEXT,
  findings         TEXT,
  visual_id        TEXT,
  status           TEXT NOT NULL CHECK (status IN ('queued', 'running', 'done', 'flagged', 'failed')),
  run_id           TEXT,
  supersedes       TEXT REFERENCES card(id),
  produced_by      TEXT,
  schema_version   TEXT NOT NULL DEFAULT '1.0',
  confidence       REAL,
  position         TEXT NOT NULL DEFAULT '{"x":0,"y":0,"dx":0,"dy":0,"pinned":false}',
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL
) STRICT;

CREATE INDEX card_board ON card(board_id, created_at);
CREATE INDEX card_parent ON card(parent_card_id);
-- A rerun inserts a new row pointing at the old one; the board shows the head
-- of each chain.
CREATE INDEX card_supersedes ON card(supersedes) WHERE supersedes IS NOT NULL;

CREATE TABLE visual (
  id             TEXT PRIMARY KEY,
  card_id        TEXT NOT NULL REFERENCES card(id) ON DELETE CASCADE,
  type           TEXT NOT NULL CHECK (type IN ('tree', 'table', 'list', 'steps', 'figure', 'image', 'chart', 'widget')),
  title          TEXT NOT NULL,
  payload        TEXT NOT NULL,
  block_index    TEXT NOT NULL,
  supersedes     TEXT REFERENCES visual(id),
  produced_by    TEXT,
  schema_version TEXT NOT NULL DEFAULT '1.0',
  created_at     TEXT NOT NULL
) STRICT;

CREATE INDEX visual_card ON visual(card_id);

-- --------------------------------------------------------------- material ---
-- Authored material. Ink has no produced_by: the user drew it.

CREATE TABLE ink (
  id         TEXT PRIMARY KEY,
  board_id   TEXT NOT NULL REFERENCES board(id) ON DELETE CASCADE,
  -- A token name, never a raw colour, so a bundle renders in the recipient's theme.
  colour     TEXT NOT NULL,
  width      REAL NOT NULL,
  points     TEXT NOT NULL,
  created_at TEXT NOT NULL
) STRICT;

CREATE INDEX ink_board ON ink(board_id);

CREATE TABLE note (
  id         TEXT PRIMARY KEY,
  board_id   TEXT NOT NULL REFERENCES board(id) ON DELETE CASCADE,
  text       TEXT NOT NULL,
  colour     TEXT NOT NULL,
  position   TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX note_board ON note(board_id);

CREATE TABLE image (
  id             TEXT PRIMARY KEY,
  board_id       TEXT NOT NULL REFERENCES board(id) ON DELETE CASCADE,
  origin         TEXT NOT NULL CHECK (origin IN ('pasted', 'generated', 'sketch_raster')),
  -- sha256 of the bytes. Stored once by hash, so a forked board never duplicates.
  blob_ref       TEXT NOT NULL,
  mime           TEXT NOT NULL,
  width          INTEGER NOT NULL,
  height         INTEGER NOT NULL,
  position       TEXT NOT NULL,
  generation     TEXT,
  source_ink_ids TEXT,
  source_note_ids TEXT,
  created_at     TEXT NOT NULL
) STRICT;

CREATE INDEX image_board ON image(board_id);
CREATE INDEX image_blob ON image(blob_ref);

-- ------------------------------------------------------------ provenance ---
-- Source and Concept are shared across every board owned by the profile and
-- survive board deletion. Doc 01 section 4.7.

CREATE TABLE source (
  id               TEXT PRIMARY KEY,
  profile_id       TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  class            TEXT NOT NULL CHECK (class IN ('web', 'regulatory', 'local_document', 'structured_query', 'user_supplied')),
  title            TEXT NOT NULL,
  locator          TEXT NOT NULL,
  site_or_issuer   TEXT,
  published_at     TEXT,
  retrieved_at     TEXT NOT NULL,
  last_verified_at TEXT,
  content_hash     TEXT,
  freshness_class  TEXT NOT NULL,
  trust_rank       INTEGER NOT NULL,
  -- Normalised locator. Two retrievals of the same page yield one Source.
  dedupe_key       TEXT NOT NULL,
  -- Doc 01 open question 4, resolved as proposed: a separate field, populated by
  -- the regulatory retriever only.
  version_ref      TEXT,
  stale            INTEGER NOT NULL DEFAULT 0 CHECK (stale IN (0, 1)),
  stale_reason     TEXT,
  created_at       TEXT NOT NULL,
  UNIQUE (profile_id, dedupe_key)
) STRICT;

CREATE INDEX source_class ON source(profile_id, class, trust_rank);
CREATE INDEX source_stale ON source(profile_id, stale) WHERE stale = 1;

CREATE TABLE passage (
  id              TEXT PRIMARY KEY,
  source_id       TEXT NOT NULL REFERENCES source(id) ON DELETE CASCADE,
  -- Verbatim as retrieved. Doc 01 open question 2, resolved as proposed:
  -- verbatim by default; a folder marked sensitive stores offsets only and its
  -- passages are blocked from export.
  text            TEXT,
  location        TEXT,
  retrieved_in_run TEXT,
  retrieved_by    TEXT NOT NULL,
  embedding_ref   TEXT,
  -- Set when the source folder is marked sensitive: text is null, offsets stand in.
  text_withheld   INTEGER NOT NULL DEFAULT 0 CHECK (text_withheld IN (0, 1)),
  created_at      TEXT NOT NULL
) STRICT;

CREATE INDEX passage_source ON passage(source_id);

CREATE TABLE citation (
  id               TEXT PRIMARY KEY,
  card_id          TEXT NOT NULL REFERENCES card(id) ON DELETE CASCADE,
  ordinal          INTEGER NOT NULL,
  passage_id       TEXT NOT NULL REFERENCES passage(id),
  -- Character offsets into Card.answer, or a block_ref when the claim lives in
  -- a Visual. Doc 01 open question 1 resolved as derived: markers are rendered
  -- from these offsets, not stored inline.
  claim_span       TEXT NOT NULL,
  binding          TEXT NOT NULL CHECK (binding IN ('answer', 'finding', 'block')),
  verifier_verdict TEXT NOT NULL DEFAULT 'unchecked'
                     CHECK (verifier_verdict IN ('supported', 'weak', 'unsupported', 'unchecked')),
  supersedes       TEXT REFERENCES citation(id),
  created_at       TEXT NOT NULL,
  UNIQUE (card_id, ordinal)
) STRICT;

CREATE INDEX citation_passage ON citation(passage_id);

-- --------------------------------------------------------------- concepts ---

CREATE TABLE concept (
  id                   TEXT PRIMARY KEY,
  profile_id           TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  term                 TEXT NOT NULL,
  aliases              TEXT,
  definition           TEXT,
  definition_card_id   TEXT,
  audience_definitions TEXT,
  doctrine_pack_id     TEXT NOT NULL REFERENCES doctrine_pack(id),
  status               TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'confirmed')),
  supersedes           TEXT REFERENCES concept(id),
  created_at           TEXT NOT NULL,
  updated_at           TEXT NOT NULL
) STRICT;

CREATE INDEX concept_term ON concept(profile_id, term);

CREATE TABLE concept_link (
  id          TEXT PRIMARY KEY,
  concept_id  TEXT NOT NULL REFERENCES concept(id) ON DELETE CASCADE,
  target_type TEXT NOT NULL CHECK (target_type IN ('card', 'visual_block', 'source', 'concept')),
  target_ref  TEXT NOT NULL,
  relation    TEXT NOT NULL CHECK (relation IN ('explains', 'mentions', 'defines', 'contradicts', 'related_to')),
  proposed_by TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'confirmed', 'rejected')),
  created_at  TEXT NOT NULL
) STRICT;

CREATE INDEX concept_link_concept ON concept_link(concept_id);
CREATE INDEX concept_link_target ON concept_link(target_type, target_ref);

-- ---------------------------------------------------------------- review ----

CREATE TABLE flag (
  id         TEXT PRIMARY KEY,
  card_id    TEXT NOT NULL REFERENCES card(id) ON DELETE CASCADE,
  rule_id    TEXT NOT NULL,
  severity   TEXT NOT NULL CHECK (severity IN ('info', 'warn', 'block')),
  target     TEXT NOT NULL,
  reason     TEXT NOT NULL,
  evidence   TEXT,
  status     TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'accepted', 'dismissed', 'fixed')),
  review_id  TEXT,
  created_at TEXT NOT NULL
) STRICT;

CREATE INDEX flag_card ON flag(card_id, status);
-- The Flags queue reads this: open flags across boards, severity then age.
CREATE INDEX flag_open ON flag(status, severity, created_at) WHERE status = 'open';
-- Doc 09 section 6: dismissals per rule over the last 30 days feed the false
-- positive rate that decides whether a rule stays enabled.
CREATE INDEX flag_rule ON flag(rule_id, status, created_at);

-- Reviews are immutable. Changing your mind inserts another Review.
CREATE TABLE review (
  id                TEXT PRIMARY KEY,
  flag_ids          TEXT NOT NULL,
  decision          TEXT NOT NULL CHECK (decision IN ('accept', 'dismiss', 'rerun', 'edit')),
  note              TEXT,
  resulting_card_id TEXT,
  decided_at        TEXT NOT NULL
) STRICT;

-- -------------------------------------------------------------- exercise ----

CREATE TABLE exercise (
  id             TEXT PRIMARY KEY,
  board_id       TEXT NOT NULL REFERENCES board(id) ON DELETE CASCADE,
  scope          TEXT NOT NULL,
  template_id    TEXT NOT NULL,
  audience_id    TEXT,
  items          TEXT NOT NULL,
  produced_by    TEXT,
  schema_version TEXT NOT NULL DEFAULT '1.0',
  created_at     TEXT NOT NULL
) STRICT;

CREATE INDEX exercise_board ON exercise(board_id);

-- Attempts stay local to the profile and are excluded from bundles by default.
CREATE TABLE attempt (
  id          TEXT PRIMARY KEY,
  exercise_id TEXT NOT NULL REFERENCES exercise(id) ON DELETE CASCADE,
  answers     TEXT NOT NULL,
  score       TEXT NOT NULL,
  taken_at    TEXT NOT NULL
) STRICT;

CREATE INDEX attempt_exercise ON attempt(exercise_id);

-- Doc 14 section 2.
CREATE TABLE learn_session (
  id         TEXT PRIMARY KEY,
  board_id   TEXT NOT NULL REFERENCES board(id) ON DELETE CASCADE,
  topic      TEXT NOT NULL,
  intake     TEXT NOT NULL DEFAULT '[]',
  plan       TEXT NOT NULL DEFAULT '[]',
  checks     TEXT NOT NULL DEFAULT '[]',
  opened     TEXT NOT NULL DEFAULT '[]',
  status     TEXT NOT NULL CHECK (status IN ('intake', 'building', 'reading', 'checking', 'ended')),
  mastery    TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX learn_session_board ON learn_session(board_id, created_at DESC);

-- ------------------------------------------------------------------ index ---
-- Doc 01 section 8. A local document chunk becomes a Passage only when cited.

CREATE TABLE index_entry (
  id                  TEXT PRIMARY KEY,
  folder_id           TEXT NOT NULL,
  document_chunk_ref  TEXT NOT NULL,
  passage_id          TEXT REFERENCES passage(id),
  content_hash        TEXT NOT NULL,
  chunk_text          TEXT NOT NULL,
  location            TEXT,
  created_at          TEXT NOT NULL
) STRICT;

CREATE INDEX index_entry_folder ON index_entry(folder_id);
CREATE INDEX index_entry_hash ON index_entry(content_hash);

-- Full text half of the hybrid search in doc 05 section 8.2. The vector half is
-- a sqlite-vec table added in the migration that lands with the local retriever,
-- so the extension is not a hard dependency before then.
CREATE VIRTUAL TABLE index_fts USING fts5(
  chunk_text,
  content = 'index_entry',
  content_rowid = 'rowid'
);

CREATE TRIGGER index_entry_ai AFTER INSERT ON index_entry BEGIN
  INSERT INTO index_fts(rowid, chunk_text) VALUES (new.rowid, new.chunk_text);
END;

CREATE TRIGGER index_entry_ad AFTER DELETE ON index_entry BEGIN
  INSERT INTO index_fts(index_fts, rowid, chunk_text) VALUES ('delete', old.rowid, old.chunk_text);
END;

CREATE TRIGGER index_entry_au AFTER UPDATE ON index_entry BEGIN
  INSERT INTO index_fts(index_fts, rowid, chunk_text) VALUES ('delete', old.rowid, old.chunk_text);
  INSERT INTO index_fts(rowid, chunk_text) VALUES (new.rowid, new.chunk_text);
END;
