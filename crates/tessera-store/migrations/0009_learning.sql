-- 0009: the knowledge map gains its learning layer. Doc 17 sections 2.1 to 2.4.
--
-- Three shapes, and only one of them touches a table in use:
--
--   1. `concept` gains six nullable columns, as ADD COLUMNs. Nothing is
--      rebuilt, and every existing row reads as a concept nobody has learned
--      anything about yet, which is what it is.
--   2. `concept_edge`, new. Doc 17 section 2.1 keeps it apart from
--      `concept_link` on purpose: a link binds a concept to content that
--      mentions it, an edge says one concept has to be understood before
--      another. Putting both in one table would make "prerequisite of" a
--      relation between a concept and a card, which is not a thing.
--   3. `mission`, new. Why the learner wants this, which is what makes a
--      lesson's difficulty and examples fit a reason rather than a syllabus.
--
-- The columns are nullable rather than defaulted because null and a value mean
-- different things here: doc 17 section 2.4's self rating prior applies "only
-- when mastery is null", so a default of 0.0 would erase the difference
-- between a concept nobody has been checked on and one they failed every check
-- on.

-- --------------------------------------------------------------- concept ---

-- Doc 17 section 2.3's six states. Left nullable rather than defaulted to
-- `unseen`, so a row written before this migration is not claimed to have been
-- seen and not seen at the same time: the projection sets it, and until it
-- does the absence is honest.
ALTER TABLE concept ADD COLUMN learning_state TEXT
  CHECK (learning_state IS NULL OR learning_state IN
    ('unseen', 'exposed', 'rated', 'checked', 'mastered', 'decayed'));

-- Doc 17 section 2.1: "0 never heard of it, 1 heard of it, 2 can explain it,
-- 3 can apply it. A claim, never evidence."
ALTER TABLE concept ADD COLUMN self_rating INTEGER
  CHECK (self_rating IS NULL OR self_rating BETWEEN 0 AND 3);

-- Doc 17 section 2.4. Evidence based, and the honesty rule lives with the
-- update: a rating can never move this above 0.5, only checks can.
ALTER TABLE concept ADD COLUMN mastery REAL
  CHECK (mastery IS NULL OR (mastery >= 0.0 AND mastery <= 1.0));

-- Doc 17 section 4's ladder: the level of the last check the learner passed.
ALTER TABLE concept ADD COLUMN difficulty_level INTEGER
  CHECK (difficulty_level IS NULL OR difficulty_level BETWEEN 1 AND 4);

-- Doc 17 section 2.3's decay is computed from this rather than scheduled, so
-- nothing has to run while the app is closed for a concept to go stale.
ALTER TABLE concept ADD COLUMN last_evidence_at TEXT;

-- Doc 17 section 2.1: the learning paths this concept belongs to, as a json
-- array of ulids. A column rather than a join table because a path is a
-- shipped list and membership is read with the concept every time it is read.
ALTER TABLE concept ADD COLUMN path_ids TEXT;

CREATE INDEX concept_learning ON concept(profile_id, learning_state);

-- ---------------------------------------------------------- concept_edge ---
-- Doc 17 section 2.1. Prerequisite structure, distinct from `concept_link`.
--
-- "Prerequisites are proposed by the Learning Planner and confirmed by the
-- learner or by a shipped path", which is doc 01 section 4.10's rule for
-- concepts applied to the edges between them: an agent proposes, a person
-- confirms.

CREATE TABLE concept_edge (
  id              TEXT PRIMARY KEY,
  from_concept_id TEXT NOT NULL REFERENCES concept(id) ON DELETE CASCADE,
  to_concept_id   TEXT NOT NULL REFERENCES concept(id) ON DELETE CASCADE,
  relation        TEXT NOT NULL CHECK (relation IN ('prerequisite_of', 'part_of', 'contrasts_with')),
  proposed_by     TEXT NOT NULL,
  status          TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'confirmed')),
  -- Doc 17 section 2.1's weight: how strongly the earlier concept is needed
  -- for the later one, which is what orders the frontier when two prerequisites
  -- are both unmet.
  weight          REAL NOT NULL DEFAULT 1.0 CHECK (weight >= 0.0 AND weight <= 1.0),
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
) STRICT;

-- One edge per pair per relation. A planner that proposes the same
-- prerequisite twice is proposing it once.
CREATE UNIQUE INDEX concept_edge_pair
  ON concept_edge(from_concept_id, to_concept_id, relation);
CREATE INDEX concept_edge_to ON concept_edge(to_concept_id, status);

-- ---------------------------------------------------------------- mission ---
-- Doc 17 section 2.1: "why the learner wants this". Every lesson is planned
-- against an active mission, so difficulty and examples fit the reason rather
-- than a syllabus.

CREATE TABLE mission (
  id                 TEXT PRIMARY KEY,
  profile_id         TEXT NOT NULL REFERENCES profile(id) ON DELETE CASCADE,
  statement          TEXT NOT NULL,
  -- A json array of concept ids: what the learner is trying to reach.
  target_concept_ids TEXT NOT NULL DEFAULT '[]',
  audience_id        TEXT,
  status             TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'done')),
  created_at         TEXT NOT NULL,
  updated_at         TEXT NOT NULL
) STRICT;

CREATE INDEX mission_profile ON mission(profile_id, status);
