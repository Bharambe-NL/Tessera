-- 0002: memory. Doc 01 v0.2, doc 05 v0.2 section 8.5, doc 15.
--
-- Doc 15 section 2 states the rule this migration exists to make checkable: a
-- prior card is context, never evidence. Three of the four changes below are
-- what lets the Verifier enforce it. `own_card` names the class of passage a
-- prior card produces, so a rule can single it out. `card.builds_on` records
-- which prior cards a card actually used, so the chain is auditable and stale
-- propagation has something to walk. `memory_enabled` lets the whole mechanism
-- be switched off per profile.
--
-- Two fields doc 01 v0.2 also lists, `Board.mode` and the LearnSession table,
-- are absent here because they landed in 0001: doc 13 had already flagged them
-- and M1 built them.

-- ------------------------------------------------------------------ card ---
-- Doc 01 section 4.4: a json array of {board_id, card_id, verified_at}. Empty
-- for every card that exists today, which is what the default says.
ALTER TABLE card ADD COLUMN builds_on TEXT NOT NULL DEFAULT '[]';

-- --------------------------------------------------------------- profile ---
-- Doc 01 section 4.16: "Boards retriever on by default", so the default is 1.
ALTER TABLE profile ADD COLUMN memory_enabled INTEGER NOT NULL DEFAULT 1
  CHECK (memory_enabled IN (0, 1));

-- ---------------------------------------------------------------- source ---
-- Doc 01 section 4.8 gains the class `own_card`.
--
-- SQLite cannot widen a CHECK constraint in place, so the table is rebuilt by
-- the procedure in the SQLite documentation: build the replacement, copy, drop
-- the original, rename. Foreign keys are off for the whole migration, set by
-- Store::migrate, because `passage.source_id` cascades on delete and dropping
-- this table with them on would take every passage in the profile with it.

CREATE TABLE source_new (
  id               TEXT PRIMARY KEY,
  profile_id       TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  -- `own_card` is a prior card of this profile's own, recalled by the boards
  -- retriever. Doc 01 section 4.8: it may never be the sole support for a
  -- numeric or regulatory claim. Doctrine ranks it below every external class.
  class            TEXT NOT NULL CHECK (class IN ('web', 'regulatory', 'local_document', 'structured_query', 'user_supplied', 'own_card')),
  title            TEXT NOT NULL,
  locator          TEXT NOT NULL,
  site_or_issuer   TEXT,
  published_at     TEXT,
  retrieved_at     TEXT NOT NULL,
  last_verified_at TEXT,
  content_hash     TEXT,
  freshness_class  TEXT NOT NULL,
  trust_rank       INTEGER NOT NULL,
  dedupe_key       TEXT NOT NULL,
  version_ref      TEXT,
  stale            INTEGER NOT NULL DEFAULT 0 CHECK (stale IN (0, 1)),
  stale_reason     TEXT,
  created_at       TEXT NOT NULL,
  UNIQUE (profile_id, dedupe_key)
) STRICT;

INSERT INTO source_new (
  id, profile_id, class, title, locator, site_or_issuer, published_at,
  retrieved_at, last_verified_at, content_hash, freshness_class, trust_rank,
  dedupe_key, version_ref, stale, stale_reason, created_at
)
SELECT
  id, profile_id, class, title, locator, site_or_issuer, published_at,
  retrieved_at, last_verified_at, content_hash, freshness_class, trust_rank,
  dedupe_key, version_ref, stale, stale_reason, created_at
FROM source;

DROP TABLE source;
ALTER TABLE source_new RENAME TO source;

CREATE INDEX source_class ON source(profile_id, class, trust_rank);
CREATE INDEX source_stale ON source(profile_id, stale) WHERE stale = 1;

-- ----------------------------------------------------------- concept link ---
-- Doc 01 section 4.11 gains the relation `builds_on`, so the Library can show
-- what a concept's later cards were built from. Rebuilt for the same reason as
-- source. Nothing references concept_link, so this rebuild is local.

CREATE TABLE concept_link_new (
  id          TEXT PRIMARY KEY,
  concept_id  TEXT NOT NULL REFERENCES concept(id) ON DELETE CASCADE,
  target_type TEXT NOT NULL CHECK (target_type IN ('card', 'visual_block', 'source', 'concept')),
  target_ref  TEXT NOT NULL,
  relation    TEXT NOT NULL CHECK (relation IN ('explains', 'mentions', 'defines', 'contradicts', 'related_to', 'builds_on')),
  proposed_by TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'confirmed', 'rejected')),
  created_at  TEXT NOT NULL
) STRICT;

INSERT INTO concept_link_new (
  id, concept_id, target_type, target_ref, relation, proposed_by, status, created_at
)
SELECT
  id, concept_id, target_type, target_ref, relation, proposed_by, status, created_at
FROM concept_link;

DROP TABLE concept_link;
ALTER TABLE concept_link_new RENAME TO concept_link;

CREATE INDEX concept_link_concept ON concept_link(concept_id);
CREATE INDEX concept_link_target ON concept_link(target_type, target_ref);
