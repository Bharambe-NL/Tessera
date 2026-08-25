"""Memory ground truth. Doc 15 section 5, joining doc 02's phase 3 metrics.

Doc 15 section 5 asks for four measurements: recall of relevant prior cards at
0.85, `own_card` as sole support after verification at 0, stale propagation to
dependent cards at 0.95, and the change in answer length when prior context
exists, reported rather than gated.

Three of those need something planted. This module plants it and writes down
what it planted, so the harness scores against a fact rather than against a
judgment made while generating.

The one design rule worth stating. Relevance is derived from `fact_ids`, which
the fact ledger fixed before any board existed, and never from anything decided
while building boards. A recall threshold measured against ground truth its own
author chose can be met by choosing generously, and 0.85 would then mean
nothing. Deriving it from the ledger means the generator cannot flatter the
retriever even by accident.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field

from .boards import Board, Card, _answer_for, _visual_for
from .corpus import Document
from .edge_cases import SOLE_SOURCE_DOC_ID
from .facts import Fact, sole_source_fact
from .questions import Question


@dataclass
class MemoryTruth:
    """What the boards retriever should and should not find."""

    #: "board_id/card_id" for every card the retriever may return.
    eligible: list[str] = field(default_factory=list)
    #: Why each excluded card is excluded, so a failure names its own cause.
    ineligible: list[dict] = field(default_factory=list)
    #: q_id -> the prior cards that state a fact the question needs.
    prior_cards: dict[str, list[str]] = field(default_factory=dict)
    #: The question whose only support is a prior card. Doc 15 section 2.
    sole_support_trap: dict | None = None
    #: A card that builds on another whose source goes stale at T3.
    stale_chain: dict | None = None

    def to_json(self) -> dict:
        return asdict(self)


def ref(board: Board, card: Card) -> str:
    return f"{board.board_id}/{card.card_id}"


#: Doc 02 section 7: "four to twelve cards" per board.
CARD_CEILING = 12


def _label(fact: Fact) -> str:
    """How a question names this fact's subject, the same way `questions` does."""
    v = fact.value
    return v.get("label") or v.get("term") or v.get("goal") or "the requirement"


def _passage_for(doc: Document | None, fact_id: str) -> str:
    """The passage in `doc` that plants `fact_id`, by id, or an empty string."""
    for passage in doc.passages if doc else []:
        if any(fid == fact_id for fid, _ in passage.plants):
            return passage.passage_id
    return ""


def _plant_card(
    board: Board,
    suffix: str,
    question: str,
    fact: Fact,
    doc: Document | None,
    ordinal: int = 1,
) -> Card:
    """Add one verified card to a board, stating `fact` and citing `doc`.

    Both memory cases need a prior card that states a particular fact, and no
    board card does: board cards are seeded from root questions, and no question
    requires a superseded fact or the held out one. Searching for a card that
    does not exist is what the first version of this module did, and it returned
    nothing twice.
    """
    card = Card(
        card_id=f"{board.board_id}-M{suffix}",
        parent_card_id=board.cards[0].card_id if board.cards else None,
        kind="follow" if board.cards else "root",
        question=question,
        depth="deep",
        answer=_answer_for(fact, ordinal),
        findings=[{"text": fact.statement, "citations": [ordinal]}],
        visual=_visual_for(fact, len(board.cards)),
        citations=[
            {
                "ordinal": ordinal,
                "source_title": doc.title if doc else "A source",
                "source_class": {
                    "regulatory": "regulatory",
                    "internal": "local_document",
                    "web": "web",
                }.get(doc.kind if doc else "web", "web"),
                "locator": doc.path if doc else "",
                "passage_id": _passage_for(doc, fact.fact_id),
                "verdict": "supported",
            }
        ],
        status="done",
        confidence=0.86,
        fact_ids=[fact.fact_id],
        memory_eligible=True,
    )
    board.cards.append(card)
    return card


def build(
    seed: int,
    facts: list[Fact],
    documents: list[Document],
    questions: list[Question],
    boards: list[Board],
) -> MemoryTruth:
    """Compute the ground truth and plant the two cases that need planting."""
    truth = MemoryTruth()

    # ---- eligibility ------------------------------------------------------
    for board in boards:
        blocked = {
            f["card_id"] for f in board.flags if f["status"] == "open" and f["severity"] == "block"
        }
        for card in board.cards:
            if card.memory_eligible:
                truth.eligible.append(ref(board, card))
                continue
            if board.trashed:
                reason = "board trashed"
            elif card.status != "done":
                reason = f"status {card.status}"
            elif card.depth not in ("deep", "research"):
                reason = f"depth {card.depth}"
            elif card.card_id in blocked:
                reason = "open block flag"
            else:
                reason = "unknown"
            truth.ineligible.append({"ref": ref(board, card), "reason": reason})

    # ---- relevant prior cards ---------------------------------------------
    # A prior card is relevant to a question when it states a fact the question
    # requires and asks something different. The second half matters: a card
    # whose question is word for word the new one is the Router's repetition
    # check, not memory, and counting it would inflate recall with a result the
    # retriever never had to earn.
    stating: dict[str, list[tuple[str, str]]] = {}
    for board in boards:
        for card in board.cards:
            if not card.memory_eligible:
                continue
            for fid in card.fact_ids:
                stating.setdefault(fid, []).append((ref(board, card), card.question))

    for q in questions:
        found: set[str] = set()
        for fid in q.required_facts:
            for card_ref, question_text in stating.get(fid, []):
                if question_text.strip().lower() != q.text.strip().lower():
                    found.add(card_ref)
        if found:
            truth.prior_cards[q.q_id] = sorted(found)

    # ---- the two planted cases --------------------------------------------
    # Three cards land on three different boards. Doc 02 section 7 says a board
    # carries four to twelve cards, so a board already at twelve is no place to
    # put one, and the stale chain needs its two ends apart anyway because the
    # retriever excludes the board it is asked from.
    by_doc = {d.doc_id: d for d in documents}
    hosts = [b for b in boards if not b.trashed and b.cards and len(b.cards) < CARD_CEILING]

    if len(hosts) >= 1:
        truth.sole_support_trap = _plant_sole_support_trap(
            seed, facts, by_doc, questions, hosts[0], truth
        )
    if len(hosts) >= 3:
        truth.stale_chain = _plant_stale_chain(facts, by_doc, hosts[1], hosts[2], truth)

    return truth


def _plant_sole_support_trap(
    seed: int,
    facts: list[Fact],
    by_doc: dict[str, Document],
    questions: list[Question],
    board: Board,
    truth: MemoryTruth,
) -> dict | None:
    """A verified card stating the fact whose only document disappears at T2.

    `corpus.build_layer_one` holds this fact out of its regulation and
    `edge_cases` gives it to one memo, which the timeline removes at T2. So from
    T2 the prior card is the only place the value appears anywhere, which is
    precisely when a Verifier is tempted to accept a card as evidence.
    """
    fact = sole_source_fact(seed, facts)
    memo = by_doc.get(SOLE_SOURCE_DOC_ID)
    if fact is None or memo is None:
        return None

    question = next((q for q in questions if fact.fact_id in q.required_facts), None)
    card = _plant_card(
        board,
        "1",
        question.text if question else f"What figure applies to {_label(fact)}?",
        fact,
        memo,
    )
    truth.eligible.append(ref(board, card))

    return {
        "q_id": question.q_id if question else None,
        "question": question.text if question else None,
        "fact_id": fact.fact_id,
        "fact_kind": fact.kind,
        "removed_document": SOLE_SOURCE_DOC_ID,
        "removed_at": "T2",
        "prior_card": ref(board, card),
        "expected": {
            # Doc 05 section 8.5 and doc 15 section 2. The prior card restates
            # the number, and restating is not evidence.
            "flag_rule_id": "own_card_sole_support",
            "severity": "block",
            "answered": False,
        },
    }


def _plant_stale_chain(
    facts: list[Fact],
    by_doc: dict[str, Document],
    origin_board: Board,
    dependent_board: Board,
    truth: MemoryTruth,
) -> dict | None:
    """A card citing a v1 value, and a card on another board that builds on it.

    Doc 02 section 5.4 already describes the first half: "a board written at T1
    that cites a v1 value should show source.stale on the affected citations at
    T3". No board did, because no question requires a superseded fact, so the
    staleness metric had nothing to measure. Doc 05 section 8.5 adds the second
    half: `verify_only` also flags the cards that build on it.

    The dependent card sits on a different board, because the boards retriever
    excludes the board it is asked from.
    """
    superseded = [f for f in facts if f.truth == "superseded"]
    v1 = by_doc.get("reg-car3-v1")
    if not superseded or v1 is None:
        return None
    fact = superseded[0]

    origin = _plant_card(
        origin_board,
        "2",
        f"What figure applies to {_label(fact)}?",
        fact,
        v1,
    )
    truth.eligible.append(ref(origin_board, origin))

    # The dependent card cites the original passage itself, which is what doc 15
    # section 2 requires of a card that builds on another. It is flagged at T3
    # because what it built on went stale, not because its own citation did.
    dependent = _plant_card(
        dependent_board,
        "3",
        f"How does {_label(fact)} affect the plan?",
        fact,
        v1,
    )
    dependent.builds_on = [
        {
            "board_id": origin_board.board_id,
            "card_id": origin.card_id,
            "verified_at": "T1",
        }
    ]
    truth.eligible.append(ref(dependent_board, dependent))

    return {
        "origin": ref(origin_board, origin),
        "dependent": ref(dependent_board, dependent),
        "fact_id": fact.fact_id,
        "superseded_by": next((f.fact_id for f in facts if f.supersedes == fact.fact_id), None),
        "stale_from": "T3",
        "expected": {
            # Both ends. The origin because its cited value changed, the
            # dependent because it built on the origin.
            "both_flagged": True,
            "flag_rule_id": "stale_source",
        },
    }


def summarise(truth: MemoryTruth) -> dict:
    return {
        "eligible_cards": len(truth.eligible),
        "ineligible_cards": len(truth.ineligible),
        "questions_with_prior_cards": len(truth.prior_cards),
        "prior_card_links": sum(len(v) for v in truth.prior_cards.values()),
        "sole_support_trap": bool(truth.sole_support_trap),
        "stale_chain": bool(truth.stale_chain),
    }
