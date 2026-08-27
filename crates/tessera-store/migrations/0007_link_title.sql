-- 0007: a link remembers the title it names, not only the words it shows.
--
-- Doc 16 section 3.1 gives PageLink a `display_text` and a target, and says an
-- unresolved link "creates the page on click". Both of those need the title,
-- and `[[Liquidity risk|the rule]]` shows "the rule": the display text is what
-- the sentence says and the title is what the link points at. With only the
-- first stored, an aliased link that could not resolve could never resolve
-- later, and clicking it would create a page called "the rule".
--
-- So every row carries the title it names, resolved or not, which also makes a
-- link row self describing: what it points at, what it says, and where it is.
--
-- ADD COLUMN, no rebuild. Existing rows are from this session's tests only, and
-- the default leaves them consistent rather than half filled.

ALTER TABLE page_link ADD COLUMN target_title TEXT NOT NULL DEFAULT '';

CREATE INDEX page_link_title ON page_link(target_kind, target_title);
