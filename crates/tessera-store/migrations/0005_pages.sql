-- 0005: doc 16's Vault entities, and the last board rebuild.
--
-- Doc 16 section 3.1 and 3.2 describe Page and PageLink; section 4 lists the
-- deltas. Three things land together because two of them are cheap and the
-- third is the expensive one:
--
--   1. `page` and `page_link`, new tables, nothing to rebuild.
--   2. `card.page_id`, an ADD COLUMN, nothing to rebuild.
--   3. `board.mode` gains `map`, which cannot be done in place.
--
-- 0004 rebuilt `board` to add `notebook` and stopped there, because doc 17 had
-- not arrived and `map` was not yet a mode anything would write. It has arrived
-- and it is adopted (BN-106), so the mode lands in this rebuild rather than in a
-- third one: doc 17 section 6's Map is "a board of `mode: map` rendered from
-- concepts", and a board table rebuilt twice for two enum values would be the
-- same mistake BN-028 recorded, made in instalments.
--
-- Pages are source of truth, not projections. `rebuild()` folds the event log
-- into card status, confidence and run cost; a page is a document the person
-- wrote, and replaying the log must never be able to rewrite one.

-- ------------------------------------------------------------------ page ---
-- Doc 16 section 3.1. The file is the export and the row is the index, so
-- `file_path` is a path inside the profile folder and the body here is what the
-- app last agreed with the file.

CREATE TABLE page (
  id               TEXT PRIMARY KEY,
  profile_id       TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  -- Renames keep the id, which is what makes a wikilink survive one.
  title            TEXT NOT NULL,
  body             TEXT NOT NULL DEFAULT '',
  -- `vault/<slug>.md`, relative to the profile folder. Doc 16 section 3.1, and
  -- subpaths from day one because doc 17 section 5 writes learning records to
  -- `vault/learning/<mission>/<date>.md`.
  file_path        TEXT NOT NULL,
  -- Set by Save as page. Doc 16 section 3.2.
  source_card_id   TEXT REFERENCES card(id) ON DELETE SET NULL,
  -- The card's citations copied as `{ordinal, passage_id}`. Doc 16 section 2.2
  -- is the reason this exists at all: a page is context, and the passages it
  -- carries are the evidence, so they are copied once and never re-derived from
  -- the page's own text.
  citations_carried TEXT NOT NULL DEFAULT '[]',
  doctrine_pack_id TEXT REFERENCES doctrine_pack(id),
  -- Doc 16 section 7 point 2: a conflicting edit keeps both, and the copy says
  -- which page it stands in for.
  supersedes       TEXT REFERENCES page(id),
  content_hash     TEXT NOT NULL DEFAULT '',
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL
) STRICT;

-- Doc 16 section 3.1: "Unique per profile, case insensitive". A separate index
-- rather than a table constraint, because the collation belongs to the
-- comparison and not to the column: the title keeps the capitals the person
-- typed and only the uniqueness check ignores them.
CREATE UNIQUE INDEX page_title ON page(profile_id, title COLLATE NOCASE);
CREATE UNIQUE INDEX page_file ON page(profile_id, file_path);
CREATE INDEX page_card ON page(source_card_id);

-- ------------------------------------------------------------- page_link ---
-- Doc 16 section 3.1. Backlinks are "select from PageLink where target_id = ?",
-- which is why the target is a column and not a scan over bodies.
--
-- `target_id` is null for an unresolved link, where the title is all there is
-- and clicking it creates the page. The kind is stored rather than inferred
-- from whether the id is null, so a link that resolved to a concept and one
-- that resolved to a page are told apart without a join.

CREATE TABLE page_link (
  id            TEXT PRIMARY KEY,
  from_page_id  TEXT NOT NULL REFERENCES page(id) ON DELETE CASCADE,
  target_kind   TEXT NOT NULL CHECK (target_kind IN ('page', 'concept', 'unresolved')),
  target_id     TEXT,
  -- What the link says in the body: `[[Title]]` shows the title, `[[Title|as]]`
  -- shows the alias, and the target is neither.
  display_text  TEXT NOT NULL,
  -- Doc 16 section 3.1's `position`: the character offset of the link in the
  -- body, so an editor can find it without re-parsing.
  position      INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL
) STRICT;

CREATE INDEX page_link_from ON page_link(from_page_id, position);
CREATE INDEX page_link_target ON page_link(target_kind, target_id);

-- ------------------------------------------------------------------ card ---
-- Doc 16 section 4: "Card gains `page_id` (set on save)". The card header shows
-- a page chip from it, and a page deleted from the vault leaves the card alone.

ALTER TABLE card ADD COLUMN page_id TEXT REFERENCES page(id) ON DELETE SET NULL;

-- ----------------------------------------------------------------- board ---
-- Doc 17 section 6: the Map is a board of `mode: map`, rendered from concept
-- rows rather than from stored cards, so viewport, events and export come free.

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
  mode                    TEXT NOT NULL DEFAULT 'explore' CHECK (mode IN ('explore', 'learn', 'notebook', 'map')),
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
