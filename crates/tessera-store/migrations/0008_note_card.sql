-- 0008: a sticky can say which card it is about.
--
-- Doc 16 section 3.6: "'Add note' from the highlight menu creates a sticky
-- attached by a dashed edge to the card, with the quote prefilled." The edge is
-- drawn from this column. Doc 01 section 4.5 gives Note a board and a position
-- and nothing else, because until now nothing wrote one at all: the table has
-- had no writer since 0001, so the attachment is added here rather than being a
-- change to something in use.
--
-- An ADD COLUMN, so nothing is rebuilt. Null is the ordinary sticky a person
-- put on the board about nothing in particular, which is what a sticky mostly
-- is; ON DELETE SET NULL keeps that sticky when the card it quoted is removed,
-- because the person's own words outlive the card they were written beside.

ALTER TABLE note ADD COLUMN card_id TEXT REFERENCES card(id) ON DELETE SET NULL;

CREATE INDEX note_card ON note(card_id);
