-- 0006: `page.content_hash` becomes `page.synced_hash`, which is what it is.
--
-- 0005 wrote the hash of the row's body into `content_hash` on every edit. That
-- is a number the row already carries, since the body is right there, and it
-- cannot answer the question the mirror actually asks.
--
-- Doc 16 section 7 point 2 wants last write wins with a conflict copy, and doc
-- 16 section 3.1 has the file and the row as two copies of one page. Deciding
-- which of them moved needs three values, not two: what the row says now, what
-- the file says now, and what the two last agreed on. The first two are the
-- bodies themselves. The third has to be stored, and it is this column.
--
-- So the column keeps its place and changes its meaning: it is the hash of the
-- text the row and the file last agreed on, written by the mirror when it
-- reconciles them and left alone by an edit. A rename rather than a new column,
-- because two hash columns where one is always derivable is how a later reader
-- ends up comparing the wrong one.

ALTER TABLE page RENAME COLUMN content_hash TO synced_hash;
