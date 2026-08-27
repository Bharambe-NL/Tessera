-- Doc 17 section 5: "The learner's sources hint from a path is passed to the
-- Planner as `must_include` locators."
--
-- A path names the sources it was written around, and a lesson planned from it
-- should read them. The hints belong to the mission rather than to the path,
-- because a mission is what a lesson is planned against and a path may be
-- loaded without one being started.
--
-- An ADD COLUMN, so no table is rebuilt. Doc 01 section 4's rule about the
-- learning columns holds here too: a column that defaults to an empty array
-- means every mission written before this migration reads as having no hints,
-- which is what they had.

ALTER TABLE mission ADD COLUMN sources_hint TEXT NOT NULL DEFAULT '[]';
