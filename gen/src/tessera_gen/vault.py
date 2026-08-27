"""The synthetic vault. Doc 16 section 5's eval line.

"Synthetic vault: 40 pages with planted facts and carried citations; questions
whose answers are only in pages; questions with no vault match."

Three kinds of page, because they test three different rules:

1. **Saved from a card.** Doc 16 section 3.2: the page carries the card's
   citations as `{ordinal, passage_id}`. Doc 16 section 2.2 is why that matters:
   the assessed package pointed the next answer's citation at the note, so two
   hops later the regulation was out of reach. A page carries the passages.

2. **Written by hand about something the corpus also says.** These are the
   pages a numeric claim must not rest on alone: the value is in the vault and
   in a document, and the Verifier's `own_card_sole_support` rule extended to
   `page` decides which one the answer may stand on.

3. **Written by hand about something no document says.** These are what a
   page-only question needs: the answer exists in the person's vault and
   nowhere else, so an answer that finds it proves the vault was read, and an
   answer that misses it proves the vault was not.

The wikilinks are the fourth thing under test. Every page links to others by
title, some link to concept terms, and a few name a page that does not exist, so
the backlink query has something to be complete about and the unresolved state
has something to hold.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field

from .boards import Board, Card
from .corpus import Document
from . import matchers
from .facts import BUILDERS, DOMAINS, Fact, LabelPool, Planting
from .questions import Question
from .rng import Rng

PAGE_COUNT = 40
#: Doc 16 section 5. Saved from a card, written about a documented fact, and
#: written about something only the vault knows.
SAVED_PAGES = 24
DOCUMENTED_PAGES = 8
PAGE_ONLY_PAGES = PAGE_COUNT - SAVED_PAGES - DOCUMENTED_PAGES

#: A handful of links that name nothing, so the unresolved state is measured
#: rather than assumed. Doc 16 section 3.1: the link is kept and creates the
#: page on click.
DANGLING_TITLES = ["A page I have not written", "Notes from the conference"]


@dataclass
class Page:
    page_id: str
    title: str
    body: str
    #: `vault/<slug>.md`, the path the mirror writes. Doc 16 section 3.1.
    file_path: str
    #: Set when the page was saved from a card. Doc 16 section 3.2.
    source_card_id: str | None = None
    board_id: str | None = None
    #: `{ordinal, passage_id}` copied from the card. Never re-derived.
    citations_carried: list[dict] = field(default_factory=list)
    #: The facts this body states, so recall from a page is checkable.
    fact_ids: list[str] = field(default_factory=list)
    #: Titles this page's wikilinks name, resolved or not.
    links_to: list[str] = field(default_factory=list)
    #: Which of the three kinds this is, so a metric can split by it.
    kind: str = "saved"

    def to_json(self) -> dict:
        return asdict(self)


@dataclass
class VaultTruth:
    pages: list[Page] = field(default_factory=list)
    #: Facts that exist only in the vault, appended to the ledger so a question
    #: naming one can be scored by the same matchers as any other.
    facts: list[Fact] = field(default_factory=list)
    #: Questions the vault answers, and questions it does not.
    questions: list[Question] = field(default_factory=list)

    def to_json(self) -> dict:
        return {
            "pages": [p.to_json() for p in self.pages],
            "vault_only_facts": [f.fact_id for f in self.facts],
            "questions": [q.q_id for q in self.questions],
        }


#: Words a title should not end on, once the long label has been cut down.
TRAILING = {"the", "a", "an", "of", "for", "to", "in", "on", "under", "and"}


def _short_title(label: str, domain: str, taken: set[str], suffix: str = "") -> str:
    """A title a person would actually type.

    The generated labels are long by design ("the confidence level for the
    internal model for a small and non-complex institution"), because a document
    has to be specific. A page title is what somebody writes at the top of their
    own note, so it is the head of that phrase, and the file name follows it.
    """
    head = str(label or domain).removeprefix("the ").split(" for ")[0].split(" under ")[0]
    words = head.strip().split()[:4]
    while words and words[-1].lower() in TRAILING:
        words.pop()
    head = " ".join(words) or domain
    title = (head[:1].upper() + head[1:]) + (f" {suffix}" if suffix else "")
    # Doc 16 section 3.1: one page per title, case insensitively. Two notes
    # about the same subject are what a numbered suffix is for.
    base, n = title, 2
    while title.lower() in taken:
        title = f"{base} {n}"
        n += 1
    taken.add(title.lower())
    return title


def _label_of(fact: Fact) -> str:
    """What the fact is about, in the words a document used."""
    label = fact.value.get("label")
    if label:
        return str(label)
    return max(fact.entity_refs, key=len) if fact.entity_refs else fact.domain


def slug(title: str) -> str:
    """The same rule as `vault::slug` in the core, because the corpus writes the
    files the app would have written."""
    out: list[str] = []
    hyphen = False
    for ch in title:
        if ch.isalnum():
            out.append(ch.lower())
            hyphen = False
        elif not hyphen and out:
            out.append("-")
            hyphen = True
    while out and out[-1] == "-":
        out.pop()
    return "".join(out) or "page"


def _link_line(titles: list[str]) -> str:
    """A sentence of wikilinks, in the two shapes doc 16 section 3.1 defines."""
    if not titles:
        return ""
    parts = []
    for i, title in enumerate(titles):
        # Every third link is aliased, so the display text and the target
        # differ often enough for a parser that confused them to be caught.
        parts.append(f"[[{title}|see also]]" if i % 3 == 2 else f"[[{title}]]")
    return "\n\nRelated: " + ", ".join(parts) + "."


def generate(
    seed: int,
    facts: list[Fact],
    documents: list[Document],
    questions: list[Question],
    boards: list[Board],
) -> VaultTruth:
    """Forty pages at T1, with the facts and questions they make answerable."""
    rng = Rng(seed, "vault")
    truth = VaultTruth()
    by_fact = {f.fact_id: f for f in facts}
    taken: set[str] = set()
    planted_collision = False

    # ---------------------------------------------------- saved from a card --
    saved_cards = [
        (board, card)
        for board in boards
        if not board.trashed
        for card in board.cards
        if card.status == "done" and card.citations and card.fact_ids
    ]
    # The stride below walks the card list in order and never reaches the last
    # boards, which are the three doc 02 line 155 ships as bundles. Doc 16's
    # pages travel with the board they were saved from, so one card on each
    # exported board is picked by hand first: without it the bundle round trip
    # carries pages on every board except the ones the corpus exports.
    exported: list[tuple[Board, Card]] = []
    on_board: set[str] = set()
    for board, card in saved_cards:
        if board.export_as_bundle and board.board_id not in on_board:
            exported.append((board, card))
            on_board.add(board.board_id)
    picks = [
        saved_cards[(i * 3) % len(saved_cards)]
        for i in range(max(min(SAVED_PAGES, len(saved_cards)) - len(exported), 0))
    ]
    picks.extend(exported)

    for index, (board, card) in enumerate(picks[:SAVED_PAGES]):
        fact = by_fact.get(card.fact_ids[0])
        if fact is None:
            continue
        title = _short_title(_label_of(fact), fact.domain, taken)
        body = (
            f"# {title}\n\n"
            f"Saved from a card I read on {board.title}.\n\n"
            f"{card.answer}\n\n"
            f"What I want to remember: {fact.statement}\n"
        )
        truth.pages.append(
            Page(
                page_id=f"P-{index + 1:03d}",
                title=title,
                body=body,
                file_path=f"vault/{slug(title)}.md",
                source_card_id=card.card_id,
                board_id=board.board_id,
                # Doc 16 section 3.2: copied, never re-derived.
                citations_carried=[
                    {"ordinal": c.get("ordinal", n + 1), "passage_id": c.get("passage_id", "")}
                    for n, c in enumerate(card.citations)
                ],
                fact_ids=[fact.fact_id],
                kind="saved",
            )
        )
        # One planted title collision, mirroring the Concept term collision doc
        # 02 section 7 plants on a bundle board. Kept on a board that carries no
        # term collision, so a failure names one merge rule rather than two, and
        # recorded on the board because the collision is a property of importing
        # this bundle rather than of the page.
        if not planted_collision and board.export_as_bundle and board.concept_collision is None:
            board.page_collision = title
            planted_collision = True

    # ------------------------------ written by hand about a documented fact --
    documented = [f for f in facts if f.planted_in and f.truth == "true"]
    for index in range(DOCUMENTED_PAGES):
        fact = documented[(index * 37) % len(documented)]
        title = _short_title(_label_of(fact), fact.domain, taken, "as I read it")
        body = (
            f"# {title}\n\n"
            f"{fact.statement}\n\n"
            "I wrote this from the rule rather than copying it, so nothing here is a citation.\n"
        )
        truth.pages.append(
            Page(
                page_id=f"P-{SAVED_PAGES + index + 1:03d}",
                title=title,
                body=body,
                file_path=f"vault/{slug(title)}.md",
                fact_ids=[fact.fact_id],
                kind="documented",
            )
        )

    # ------------------------------- written by hand about nothing on record --
    for index in range(PAGE_ONLY_PAGES):
        fact_rng = rng.derive(f"only-{index}")
        domain = DOMAINS[index % len(DOMAINS)]
        labels = LabelPool(fact_rng.derive("labels"), domain, 4)
        statement, value, refs = BUILDERS["number"](fact_rng, domain, labels)
        # "Only in the vault" has to be true, not asserted. A label pool shared
        # with the documented facts can hand out a value some document already
        # states, and a page-only question whose answer is also in the corpus
        # measures nothing.
        statement, value = _found_nowhere_else(statement, value, documents)
        # From this iteration's own value. The first version read `fact`, which
        # was still bound to the previous loop's, so every page-only title named
        # a different subject from the one its body stated.
        title = _short_title(str(value.get("label", "")), domain, taken, "from the session")
        page_id = f"P-{SAVED_PAGES + DOCUMENTED_PAGES + index + 1:03d}"
        file_path = f"vault/{slug(title)}.md"
        fact = Fact(
            fact_id=f"VF-{index + 1:04d}",
            domain=domain,
            statement=statement,
            kind="number",
            value=value,
            entity_refs=list(refs),
            truth="true",
            # The page is the only place this is written down, which is what
            # makes a question about it answerable from the vault and nowhere
            # else.
            planted_in=[Planting(doc_id=file_path, passage_id=page_id, fidelity="exact")],
        )
        truth.facts.append(fact)
        body = (
            f"# {title}\n\n"
            f"{statement}\n\n"
            "Nobody has written this down but me.\n"
        )
        truth.pages.append(
            Page(
                page_id=page_id,
                title=title,
                body=body,
                file_path=file_path,
                fact_ids=[fact.fact_id],
                kind="page_only",
            )
        )

    _link(rng, truth)
    _ask(rng, truth, questions)
    return truth


def _found_nowhere_else(statement: str, value: dict, documents: list[Document]) -> tuple[str, dict]:
    """Move a value until no document in the corpus states it.

    Deterministic: the same corpus and the same starting value always end in the
    same place. The step is a hundredth, which no generated subject uses, so one
    move is almost always enough and the result still reads like a figure
    somebody wrote down.
    """
    for attempt in range(50):
        if not any(
            matchers.matches("number", value, passage.text)
            for document in documents
            for passage in document.passages
        ):
            return statement, value
        before = str(value.get("amount", "0"))
        try:
            moved = f"{float(before) + 0.01 * (attempt + 1):g}"
        except ValueError:
            return statement, value
        statement = statement.replace(before, moved)
        value = dict(value, amount=moved)
    return statement, value


def _link(rng: Rng, truth: VaultTruth) -> None:
    """Wikilinks between the pages, and a few that name nothing.

    Deterministic and asymmetric: page n links forward, so the number of links
    into a page differs from the number out of it and a backlink count that
    mixed the two directions would be caught.
    """
    titles = [p.title for p in truth.pages]
    for index, page in enumerate(truth.pages):
        step = 1 + (index % 3)
        targets = [titles[(index + step * k) % len(titles)] for k in range(1, 1 + (index % 4))]
        targets = [t for t in targets if t != page.title]
        if index % 9 == 4:
            targets.append(DANGLING_TITLES[index % len(DANGLING_TITLES)])
        if not targets:
            continue
        page.links_to = targets
        page.body = page.body.rstrip() + _link_line(targets) + "\n"


def _ask(rng: Rng, truth: VaultTruth, questions: list[Question]) -> None:
    """Two families. Doc 16 section 5.

    A page-only question can be answered from the vault and nowhere else. A
    no-vault question is answerable from the corpus while the vault holds
    nothing about it, which is what the notebook's ungrounded state is measured
    on at 12d.
    """
    for index, fact in enumerate(truth.facts):
        page = next(p for p in truth.pages if fact.fact_id in p.fact_ids)
        label = fact.value.get("label")
        if not label:
            label = fact.entity_refs[0] if fact.entity_refs else "that figure"
        truth.questions.append(
            Question(
                q_id=f"QV-{index + 1:04d}",
                text=f"What did I write down about {label}?",
                domain=fact.domain,
                depth_expected="deep",
                audience_id=None,
                required_facts=[fact.fact_id],
                required_sources=[page.file_path],
                forbidden_facts=[],
                expected_visual="list",
                expected_flags=[],
                edge_case_ids=["page_only"],
            )
        )

    # The other half: questions the corpus answers and the vault says nothing
    # about. Taken from the existing set rather than invented, so the only thing
    # that differs is the vault, which is what the measurement is about.
    for index, question in enumerate(questions[:: max(1, len(questions) // 8)][:8]):
        truth.questions.append(
            Question(
                q_id=f"QN-{index + 1:04d}",
                text=question.text,
                domain=question.domain,
                depth_expected=question.depth_expected,
                audience_id=question.audience_id,
                required_facts=list(question.required_facts),
                required_sources=list(question.required_sources),
                forbidden_facts=list(question.forbidden_facts),
                expected_visual=question.expected_visual,
                expected_flags=list(question.expected_flags),
                edge_case_ids=["no_vault_match"],
            )
        )


def summarise(truth: VaultTruth) -> dict:
    kinds: dict[str, int] = {}
    for page in truth.pages:
        kinds[page.kind] = kinds.get(page.kind, 0) + 1
    return {
        "total": len(truth.pages),
        "by_kind": kinds,
        "carried_citations": sum(len(p.citations_carried) for p in truth.pages),
        "links": sum(len(p.links_to) for p in truth.pages),
        "vault_only_facts": len(truth.facts),
        "questions": len(truth.questions),
    }
