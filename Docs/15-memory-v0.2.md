# 15. Memory v0.2

Register: working. Amends 01 and 05.

## 1. The five memories

| Memory | Where it lives | How it enters a call | Who writes it |
|---|---|---|---|
| Profile | Profile row | Appended to every system prompt | The user |
| Semantic | Concept, ConceptLink | Planner entity resolution; audience definitions | Agents propose, user confirms |
| Evidence | Source, Passage | Retrievers; deduplicated; re-verified for staleness | Retrievers |
| Episodic | The boards retriever over the profile's own verified cards | As passages of class own_card, context only | The pipeline, automatically |
| Learner and team | LearnSession mastery; DoctrinePack rulings | Tutor check selection; Verifier rules | Tutor; the user editing the pack |

## 2. The rule

A prior card is context, never evidence. Cards may reuse framing, vocabulary, and structure from earlier work, and they show "Builds on" so the reader can trace it. Any number, date, obligation, or rule cited in a new card must cite the original passage, which the boards passage carries in its digest. The Verifier blocks `own_card` as sole support. This stops boards citing boards with the real source drifting out of reach and possibly stale.

## 3. Eligibility

Only verified cards remember: done, deep or research, no open block flags, board not trashed. Fast cards and flagged cards are excluded. Memory can be switched off per profile; a future per-board "private" toggle excludes a board from being recalled elsewhere.

## 4. Surfaces

Card header chip "builds on n" (click opens the earliest prior card); "How this was built" lists the prior cards; Library concept detail shows `builds_on` links; stale propagation reaches dependent cards.

## 5. Eval additions (02)

Synthetic boards at T1 provide prior cards; questions at T2 measure: recall of relevant prior cards 0.85; own_card sole support after verification 0; stale propagation to dependent cards 0.95; answer length reduction when prior context exists (should shorten, reported).

## 6. Prototype status

The prototype implements episodic memory with a keyword overlap retriever over other boards' non-fast done cards, a Profile toggle, the chip, and the context-only instruction in the prompt. Embeddings, eligibility by flags, and the Verifier block are spec only.
