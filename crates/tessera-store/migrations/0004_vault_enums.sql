-- 0004: the enum values doc 16's Vault layer needs, added before anything writes them.
--
-- Doc 16 section 4 lists its data model deltas. Three are CHECK constraint
-- widenings: `source.class` gains `page`, `visual.type` gains `flow` and
-- `stats`, and `board.mode` gains `notebook`. Doc 16 section 5 builds the code
-- that writes them well after phase 7, and none of it exists yet.
--
-- They land now anyway, for one reason. SQLite cannot widen a CHECK in place,
-- so each of these means rebuilding its table, and BN-028 recorded what that
-- costs on `source`: `passage.source_id` cascades on delete, so dropping the
-- table with foreign keys enabled deletes every passage in the profile and
-- every citation with them. That rebuild is done once here rather than a second
-- time later, which is the whole saving. The enum value itself costs nothing:
-- BN-029 settled that widening an enum leaves every existing row valid, and
-- nothing writes `page`, `flow`, `stats` or `notebook` until the Vault layer
-- builds the code that does.

-- ---------------------------------------------------------------- source ---
-- Doc 16 section 3.3: a page's source class, trust rank 4 in the finance pack,
-- below external sources and above own_card. Doc 16 section 3.3 also extends
-- `own_card_sole_support` to it: a page is context, and the citations it
-- carries from the card it was saved from are the evidence.

CREATE TABLE source_new (
  id               TEXT PRIMARY KEY,
  profile_id       TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  class            TEXT NOT NULL CHECK (class IN ('web', 'regulatory', 'local_document', 'structured_query', 'user_supplied', 'own_card', 'page')),
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

-- ---------------------------------------------------------------- visual ---
-- Doc 16 section 3.5. `flow` is nodes plus edges, which the tree type cannot
-- express because a tree has no cycles and no cross links. `stats` is up to six
-- large numerals, each cited, because a tile without a citation is a number
-- with nobody standing behind it.

CREATE TABLE visual_new (
  id             TEXT PRIMARY KEY,
  card_id        TEXT NOT NULL REFERENCES card(id) ON DELETE CASCADE,
  type           TEXT NOT NULL CHECK (type IN ('tree', 'table', 'list', 'steps', 'figure', 'image', 'chart', 'widget', 'flow', 'stats')),
  title          TEXT NOT NULL,
  payload        TEXT NOT NULL,
  block_index    TEXT NOT NULL,
  supersedes     TEXT REFERENCES visual(id),
  produced_by    TEXT,
  schema_version TEXT NOT NULL DEFAULT '1.0',
  created_at     TEXT NOT NULL
) STRICT;

INSERT INTO visual_new (
  id, card_id, type, title, payload, block_index, supersedes, produced_by,
  schema_version, created_at
)
SELECT
  id, card_id, type, title, payload, block_index, supersedes, produced_by,
  schema_version, created_at
FROM visual;

DROP TABLE visual;
ALTER TABLE visual_new RENAME TO visual;

CREATE INDEX visual_card ON visual(card_id);

-- ----------------------------------------------------------------- board ---
-- Doc 16 section 3.4: "Sessions are boards of `mode: notebook` so history,
-- events, memory, and export come free."

CREATE TABLE board_new (
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
  mode                    TEXT NOT NULL DEFAULT 'explore' CHECK (mode IN ('explore', 'learn', 'notebook')),
  status                  TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'trashed')),
  trashed_at              TEXT,
  created_at              TEXT NOT NULL,
  updated_at              TEXT NOT NULL
) STRICT;

INSERT INTO board_new (
  id, profile_id, title, named_by_user, doctrine_pack_id, context, seed_label,
  parent_board_id, forked_from_bundle_id, viewport, default_depth,
  default_model_policy_id, mode, status, trashed_at, created_at, updated_at
)
SELECT
  id, profile_id, title, named_by_user, doctrine_pack_id, context, seed_label,
  parent_board_id, forked_from_bundle_id, viewport, default_depth,
  default_model_policy_id, mode, status, trashed_at, created_at, updated_at
FROM board;

DROP TABLE board;
ALTER TABLE board_new RENAME TO board;

CREATE INDEX board_profile_status ON board(profile_id, status, updated_at DESC);
