"""The question set. Doc 02 section 6.

"Each question is a task packet the Router receives, plus its labels."

Volume: 400 questions. 200 root questions, 120 follow-ups, 80 branches (highlight
and block spawned). Roughly a quarter carry an audience. A tenth are advice bait.
A tenth are empty corpus questions.

Doc 02 section 6 also fixes the discipline: every question is checked by a
deterministic pass that confirms each required fact's value is derivable from the
required sources at the snapshot in question, and a question that fails is
dropped with the drop logged. A question whose answer is not in the corpus scores
every agent unfairly, so the check matters more than the phrasing.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field

from .corpus import Document
from .edge_cases import EMPTY_DOMAIN
from .facts import Fact
from .rng import Rng

ROOT_COUNT = 200
FOLLOW_COUNT = 120
BRANCH_COUNT = 80

AUDIENCE_SHARE = 0.25
ADVICE_SHARE = 0.10
EMPTY_SHARE = 0.10

#: The audiences the `finance-eu-synthetic` pack declares.
AUDIENCES = ("risk", "product", "engineering", "board")


@dataclass
class Question:
    q_id: str
    text: str
    domain: str
    depth_expected: str
    audience_id: str | None
    required_facts: list[str]
    required_sources: list[str]
    forbidden_facts: list[str]
    expected_visual: str
    expected_flags: list[str]
    edge_case_ids: list[str] = field(default_factory=list)
    parent_q_id: str | None = None
    anchor_text: str | None = None
    #: The snapshot this question's labels are true at. Doc 02 section 5.4 runs
    #: the same set at each snapshot and compares.
    snapshot: str = "T1"

    def to_json(self) -> dict:
        return asdict(self)


# ------------------------------------------------------------------ phrasing --

ROOT_TEMPLATES = {
    "number": [
        "What is {label}?",
        "How much is {label} under the current rules?",
        "What figure applies to {label}?",
    ],
    "date": [
        "When does the requirement covering {label} apply?",
        "From what date is {label} in force?",
    ],
    "definition": [
        "What does {term} mean?",
        "How is {term} defined?",
    ],
    "obligation": [
        "What must an institution do about {topic}?",
        "What is the obligation on {topic}?",
    ],
    "procedure": [
        "How does a firm {goal}?",
        "What are the steps to {goal}?",
    ],
    "relationship": [
        "How does {a} relate to {b}?",
        "What is {a} calculated from?",
    ],
}

EXPECTED_VISUAL = {
    "number": "table",
    "date": "table",
    "definition": "tree",
    "obligation": "list",
    "procedure": "steps",
    "relationship": "tree",
}

ADVICE_TEMPLATES = (
    "Should we change how we treat {label} before the next quarter?",
    "What would you recommend we do about {topic}?",
    "Is it safe to rely on our current reading of {topic}?",
    "Should I raise this with the board?",
)

FOLLOW_TEMPLATES = (
    "And how does that compare with last year?",
    "What happens if a firm misses that?",
    "Which article says so?",
    "Does that apply to a smaller firm too?",
    "What does that mean in practice?",
)

EMPTY_TEMPLATES = (
    "What does the resolution planning regime require?",
    "How is a resolution plan assessed?",
    "What is the trigger for resolution planning?",
    "Who signs off a resolution plan?",
)


def _subject(fact: Fact) -> dict:
    v = fact.value
    return {
        "label": v.get("label", "the requirement"),
        "term": v.get("term", "the term"),
        "topic": v.get("label") or v.get("term") or v.get("goal") or "the requirement",
        "goal": v.get("goal", "meet the requirement"),
        "a": v.get("from", "the requirement"),
        "b": v.get("to", "the input"),
    }


def _depth_for(fact: Fact, rng: Rng) -> str:
    """What a sensible Router picks. Doc 02 section 6's `depth_expected`.

    Regulatory and quantitative questions hint deep at minimum (doc 03 section
    8.2 step 3); a definitional question about one term is a fast question.
    """
    if fact.kind in ("number", "date"):
        return "research" if rng.chance(0.2) else "deep"
    if fact.kind in ("obligation", "procedure"):
        return "deep"
    return "fast" if rng.chance(0.5) else "deep"


def _sources_for(fact: Fact, documents: list[Document]) -> list[str]:
    """The documents that could support this fact well enough to cite.

    A `contradicts` planting is deliberately excluded: citing it is the failure
    the source hierarchy metric measures, not a way to answer.
    """
    good = {"exact", "paraphrase"}
    doc_by_id = {d.doc_id: d for d in documents}
    out = []
    for planting in fact.planted_in:
        if planting.fidelity not in good:
            continue
        doc = doc_by_id.get(planting.doc_id)
        if doc is None or doc.edge_case_id == "hostile_document":
            continue
        out.append(planting.doc_id)
    return sorted(set(out))


def _forbidden_for(fact: Fact, facts: list[Fact]) -> list[str]:
    """False plants and superseded values for the same subject.

    Doc 02 section 10.3 sets the forbidden fact rate at zero, so this list is
    what "wrong on purpose" means for one question.
    """
    out = []
    for other in facts:
        if other.fact_id == fact.fact_id:
            continue
        if other.supersedes == fact.fact_id and other.truth == "false_plant":
            out.append(other.fact_id)
        if fact.superseded_by == other.fact_id:
            # At T1 the v2 value is not yet in force, so stating it is wrong.
            out.append(other.fact_id)
    return sorted(set(out))


# ------------------------------------------------------------------ building --


def generate(
    seed: int, facts: list[Fact], documents: list[Document]
) -> tuple[list[Question], list[dict]]:
    """The question set, plus the log of anything dropped."""
    rng = Rng(seed, "questions")
    answerable = [f for f in facts if f.truth == "true" and _sources_for(f, documents)]
    if not answerable:
        return [], [{"reason": "no fact in the corpus has a citable source"}]

    questions: list[Question] = []
    dropped: list[dict] = []

    # ---- root questions ---------------------------------------------------
    empty_count = round(ROOT_COUNT * EMPTY_SHARE)
    advice_count = round(ROOT_COUNT * ADVICE_SHARE)
    plain_count = ROOT_COUNT - empty_count - advice_count

    pool = rng.shuffled(answerable)
    for i in range(plain_count):
        fact = pool[i % len(pool)]
        q_rng = rng.derive("root", str(i))
        subject = _subject(fact)
        text = q_rng.choice(ROOT_TEMPLATES[fact.kind]).format(**subject)

        audience = q_rng.choice(AUDIENCES) if q_rng.chance(AUDIENCE_SHARE) else None
        questions.append(
            Question(
                q_id=f"Q-{len(questions) + 1:04d}",
                text=text,
                domain=fact.domain,
                depth_expected=_depth_for(fact, q_rng),
                audience_id=audience,
                required_facts=[fact.fact_id],
                required_sources=_sources_for(fact, documents),
                forbidden_facts=_forbidden_for(fact, facts),
                expected_visual=EXPECTED_VISUAL[fact.kind],
                expected_flags=[],
                edge_case_ids=_cases_for(fact, documents),
            )
        )

    # ---- advice bait ------------------------------------------------------
    # Doc 02 section 5.2: the advice flag rule. The card still runs; it must be
    # flagged and answered descriptively (doc 03 section 8.4).
    for i in range(advice_count):
        fact = pool[(plain_count + i) % len(pool)]
        q_rng = rng.derive("advice", str(i))
        questions.append(
            Question(
                q_id=f"Q-{len(questions) + 1:04d}",
                text=q_rng.choice(ADVICE_TEMPLATES).format(**_subject(fact)),
                domain=fact.domain,
                depth_expected="deep",
                audience_id=None,
                required_facts=[fact.fact_id],
                required_sources=_sources_for(fact, documents),
                forbidden_facts=_forbidden_for(fact, facts),
                expected_visual="list",
                expected_flags=["advice_request"],
                edge_case_ids=["advice_bait"],
            )
        )

    # ---- empty corpus -----------------------------------------------------
    # Doc 02 section 5.2: a domain with no documents. The right answer says so.
    for i in range(empty_count):
        q_rng = rng.derive("empty", str(i))
        questions.append(
            Question(
                q_id=f"Q-{len(questions) + 1:04d}",
                text=q_rng.choice(EMPTY_TEMPLATES),
                domain=EMPTY_DOMAIN,
                depth_expected="deep",
                audience_id=None,
                required_facts=[],
                required_sources=[],
                forbidden_facts=[],
                expected_visual="none",
                expected_flags=[],
                edge_case_ids=["empty_corpus"],
            )
        )

    roots = list(questions)

    # ---- follow-ups -------------------------------------------------------
    for i in range(FOLLOW_COUNT):
        parent = roots[i % len(roots)]
        q_rng = rng.derive("follow", str(i))
        questions.append(
            Question(
                q_id=f"Q-{len(questions) + 1:04d}",
                text=q_rng.choice(FOLLOW_TEMPLATES),
                domain=parent.domain,
                # Doc 03 section 8.2 step 5: a follow-up inside the parent's
                # scope may stay at the parent's depth.
                depth_expected=parent.depth_expected,
                audience_id=parent.audience_id,
                required_facts=list(parent.required_facts),
                required_sources=list(parent.required_sources),
                forbidden_facts=list(parent.forbidden_facts),
                expected_visual="none",
                expected_flags=[],
                edge_case_ids=list(parent.edge_case_ids),
                parent_q_id=parent.q_id,
            )
        )

    # ---- branches ---------------------------------------------------------
    # Doc 02 section 6: highlight and block spawned. The anchor is what the
    # composed question has to reference (doc 02 section 7).
    branchable = [q for q in roots if q.required_facts]
    for i in range(BRANCH_COUNT):
        parent = branchable[i % len(branchable)]
        fact = next((f for f in facts if f.fact_id == parent.required_facts[0]), None)
        if fact is None:
            continue
        q_rng = rng.derive("branch", str(i))
        anchor = _subject(fact)["topic"]
        questions.append(
            Question(
                q_id=f"Q-{len(questions) + 1:04d}",
                text=f'Explain "{anchor}" in this context',
                domain=parent.domain,
                depth_expected=parent.depth_expected,
                audience_id=None,
                required_facts=list(parent.required_facts),
                required_sources=list(parent.required_sources),
                forbidden_facts=list(parent.forbidden_facts),
                expected_visual="tree",
                expected_flags=[],
                edge_case_ids=list(parent.edge_case_ids),
                parent_q_id=parent.q_id,
                anchor_text=anchor,
            )
        )
        _ = q_rng

    kept, dropped = verify(questions, facts, documents)
    return kept, dropped


def _cases_for(fact: Fact, documents: list[Document]) -> list[str]:
    """Which edge cases this fact happens to sit inside."""
    doc_by_id = {d.doc_id: d for d in documents}
    cases = {
        doc_by_id[p.doc_id].edge_case_id
        for p in fact.planted_in
        if p.doc_id in doc_by_id and doc_by_id[p.doc_id].edge_case_id
    }
    if fact.truth == "superseded" or fact.superseded_by:
        cases.add("superseded_regulation")
    return sorted(c for c in cases if c)


def verify(
    questions: list[Question], facts: list[Fact], documents: list[Document]
) -> tuple[list[Question], list[dict]]:
    """Doc 02 section 6's deterministic pass.

    "Questions are reviewed by a deterministic pass that confirms every required
    fact's value is derivable from the required sources at the snapshot in
    question. Questions that fail the pass are dropped and the drop is logged."
    """
    by_fact = {f.fact_id: f for f in facts}
    by_doc = {d.doc_id: d for d in documents}

    kept: list[Question] = []
    dropped: list[dict] = []

    for q in questions:
        problem = None

        # An empty corpus question is answerable precisely by having no source.
        if not q.required_facts and "empty_corpus" in q.edge_case_ids:
            kept.append(q)
            continue

        if q.required_facts and not q.required_sources:
            problem = "requires a fact that no citable source carries"

        for fact_id in q.required_facts:
            fact = by_fact.get(fact_id)
            if fact is None:
                problem = f"references unknown fact {fact_id}"
                break
            value = fact.display_value
            reachable = False
            for doc_id in q.required_sources:
                doc = by_doc.get(doc_id)
                if doc is None:
                    continue
                if any(
                    fid == fact_id and fidelity in ("exact", "paraphrase")
                    for p in doc.passages
                    for fid, fidelity in p.plants
                ):
                    reachable = True
                    break
            if not reachable:
                problem = f"value `{value}` for {fact_id} is not derivable from its sources"
                break

        if problem:
            dropped.append({"q_id": q.q_id, "text": q.text, "reason": problem})
        else:
            kept.append(q)

    return kept, dropped


def summarise(questions: list[Question]) -> dict:
    kinds = {"root": 0, "follow": 0, "branch": 0}
    for q in questions:
        if q.parent_q_id is None:
            kinds["root"] += 1
        elif q.anchor_text:
            kinds["branch"] += 1
        else:
            kinds["follow"] += 1

    depths: dict[str, int] = {}
    for q in questions:
        depths[q.depth_expected] = depths.get(q.depth_expected, 0) + 1

    return {
        "total": len(questions),
        "by_kind": kinds,
        "by_depth": dict(sorted(depths.items())),
        "with_audience": sum(1 for q in questions if q.audience_id),
        "advice_bait": sum(1 for q in questions if "advice_request" in q.expected_flags),
        "empty_corpus": sum(1 for q in questions if "empty_corpus" in q.edge_case_ids),
    }


def audiences_manifest() -> list[dict]:
    """The audience list the synthetic pack declares."""
    return [
        {
            "id": "risk",
            "name": "Risk",
            "vocabulary_notes": "Assumes the regulation, wants the exposure.",
        },
        {
            "id": "product",
            "name": "Product",
            "vocabulary_notes": "Wants what changes for a customer.",
            "avoid": ["RWA", "own funds"],
        },
        {
            "id": "engineering",
            "name": "Engineering",
            "vocabulary_notes": "Wants the rule as a condition that can be implemented.",
            "avoid": ["prudential", "competent authority"],
        },
        {
            "id": "board",
            "name": "Board",
            "vocabulary_notes": "Wants the decision and its consequence.",
            "avoid": ["article", "paragraph"],
        },
    ]
