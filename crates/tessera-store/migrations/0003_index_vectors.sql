-- 0003: the vector half of hybrid retrieval. Doc 05 section 8.2, doc 10 section 3.
--
-- The lexical half already exists: `index_fts` and its triggers landed in 0001.
-- What was missing is somewhere to put an embedding, and enough about it to
-- know later whether two rows can be compared at all.
--
-- `embedding_model` is on the row rather than in a settings table on purpose.
-- Doc 10 section 17 question 2 leaves the model choice open until the recall
-- numbers decide it, which means the model will change at least once. Two
-- vector spaces mixed in one column produce no error and no crash: they produce
-- cosine similarities that are simply meaningless, and a recall number nobody
-- can explain. Recording the model per row means a re-index after a model
-- change is detectable rather than assumed.

ALTER TABLE index_entry ADD COLUMN embedding BLOB;
ALTER TABLE index_entry ADD COLUMN embedding_model TEXT;
ALTER TABLE index_entry ADD COLUMN embedding_dimensions INTEGER;

-- The scan for entries that still need a vector, and for entries left behind by
-- a model change.
CREATE INDEX index_entry_embedding_model
  ON index_entry(folder_id, embedding_model);

-- Which folders are indexed, where they live on disk, and whether their
-- contents may leave the machine. Doc 05 section 8.2: "Excluded folders are
-- never opened." Doc 01 section 4.9 as resolved: a sensitive folder's passages
-- store offsets rather than verbatim text, and are blocked from export.
CREATE TABLE watched_folder (
  id            TEXT PRIMARY KEY,
  profile_id    TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  root          TEXT NOT NULL,
  label         TEXT NOT NULL,
  sensitive     INTEGER NOT NULL DEFAULT 0 CHECK (sensitive IN (0, 1)),
  -- Doc 10 section 3: provider embeddings are opt in per folder, and doc 10
  -- section 15 requires the Retrievers page to say, per folder, whether chunk
  -- text leaves the machine.
  embeddings    TEXT NOT NULL DEFAULT 'local' CHECK (embeddings IN ('local', 'provider')),
  last_indexed_at TEXT,
  created_at    TEXT NOT NULL,
  UNIQUE (profile_id, root)
) STRICT;

CREATE INDEX watched_folder_profile ON watched_folder(profile_id);

-- Files that could not be read, so the Profile can say which and why rather
-- than leaving a document that silently vanished. Doc 05 sections 10 and 11.
-- One row per path: the newest attempt replaces the last.
CREATE TABLE index_error (
  folder_id  TEXT NOT NULL REFERENCES watched_folder(id) ON DELETE CASCADE,
  path       TEXT NOT NULL,
  kind       TEXT NOT NULL,
  detail     TEXT NOT NULL,
  noticed_at TEXT NOT NULL,
  PRIMARY KEY (folder_id, path)
) STRICT;
