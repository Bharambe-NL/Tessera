"""The fact ledger. Doc 02 section 3.

"The unit of ground truth is a fact."

This is the whole reason the corpus is synthetic. Doc 02 section 1: the Verifier's
job is to catch unsupported claims, wrong citations, stale sources and advice
language, and none of those can be measured on a real corpus without a human
labelling every claim. Here the labels come for free, because the generator
planted the facts, the contradictions and the traps on purpose.

Target volume is 600 facts across 4 domains, roughly 40 percent numbers and dates,
30 percent obligations and procedures, 30 percent definitions and relationships.
Numbers and dates are over represented because they are where citation binding is
most likely to fail and where the finance pack's flag rules bite hardest.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field
from typing import Literal

from .entities import DOMAINS, REGULATION_FOR_DOMAIN, firms_for
from .rng import Rng

FactKind = Literal["number", "date", "definition", "obligation", "relationship", "procedure"]
Truth = Literal["true", "superseded", "false_plant"]
Fidelity = Literal["exact", "paraphrase", "partial", "contradicts"]

#: Doc 02 section 3's mix.
KIND_WEIGHTS: tuple[tuple[FactKind, float], ...] = (
    ("number", 0.26),
    ("date", 0.14),
    ("obligation", 0.18),
    ("procedure", 0.12),
    ("definition", 0.16),
    ("relationship", 0.14),
)

TOTAL_FACTS = 600

#: Doc 02 section 5.2: CAR3 v1 (2024) and v2 (2026) both exist, and 30 facts
#: change value between them.
SUPERSEDED_COUNT = 30

#: Doc 02 section 3: facts that are wrong on purpose. A press page that misquotes
#: a threshold, an internal memo with a typo in a date. A correct answer never
#: cites them for the wrong value; a correct Verifier flags a card that does.
FALSE_PLANT_COUNT = 40


@dataclass
class Planting:
    doc_id: str
    passage_id: str
    fidelity: Fidelity


@dataclass
class Fact:
    fact_id: str
    domain: str
    statement: str
    kind: FactKind
    value: dict
    entity_refs: list[str]
    truth: Truth
    superseded_by: str | None = None
    planted_in: list[Planting] = field(default_factory=list)
    concept_ids: list[str] = field(default_factory=list)
    #: The regulation article this fact belongs to, so the regulatory text and
    #: the questions agree on where to find it.
    article: int | None = None
    #: Set on a v2 fact: the v1 fact it replaces.
    supersedes: str | None = None

    def to_json(self) -> dict:
        out = asdict(self)
        out["planted_in"] = [asdict(p) for p in self.planted_in]
        return out

    @property
    def display_value(self) -> str:
        """The value as a source would write it. What a matcher looks for."""
        v = self.value
        if "amount" in v:
            unit = v.get("unit", "")
            return f"{v['amount']} {unit}".strip()
        if "date" in v:
            return str(v["date"])
        if "count" in v:
            return str(v["count"])
        return str(v.get("text", ""))


# --------------------------------------------------------------- vocabulary --

CONCEPTS = {
    "capital": [
        ("capital-buffer", "capital buffer"),
        ("risk-weighted-assets", "risk weighted assets"),
        ("trading-book", "trading book"),
        ("leverage-ratio", "leverage ratio"),
        ("own-funds", "own funds"),
    ],
    "payments": [
        ("safeguarding", "safeguarding"),
        ("strong-authentication", "strong customer authentication"),
        ("payment-institution", "payment institution"),
        ("incident-report", "incident report"),
        ("account-information", "account information service"),
    ],
    "outsourcing": [
        ("critical-function", "critical or important function"),
        ("outsourcing-register", "outsourcing register"),
        ("exit-plan", "exit plan"),
        ("sub-outsourcing", "sub outsourcing"),
        ("service-provider", "service provider"),
    ],
    "model-risk": [
        ("model-validation", "model validation"),
        ("backtesting", "backtesting"),
        ("model-inventory", "model inventory"),
        ("internal-model", "internal model"),
        ("model-owner", "model owner"),
    ],
}

#: (label, unit, low, high). The numbers a domain talks about.
NUMERIC_SUBJECTS = {
    "capital": [
        ("the capital conservation buffer", "%", 15, 40, 10),
        ("the countercyclical buffer", "%", 0, 25, 10),
        ("the minimum own funds requirement", "%", 60, 105, 10),
        ("the leverage ratio floor", "%", 25, 50, 10),
        ("the trading book threshold", "million EUR", 20, 500, 1),
        ("the large exposure limit", "% of own funds", 10, 25, 1),
    ],
    "payments": [
        ("the safeguarding threshold", "thousand EUR", 50, 900, 1),
        ("the incident reporting window", "hours", 2, 72, 1),
        ("the authentication exemption limit", "EUR", 30, 500, 1),
        ("the refund deadline", "business days", 1, 20, 1),
        ("the initial capital requirement", "thousand EUR", 20, 350, 1),
    ],
    "outsourcing": [
        ("the register update interval", "days", 15, 180, 1),
        ("the notification period before an outsourcing starts", "days", 20, 120, 1),
        ("the exit plan review interval", "months", 6, 36, 1),
        ("the sub outsourcing notice period", "days", 10, 90, 1),
    ],
    "model-risk": [
        ("the backtesting exception threshold", "exceptions per year", 4, 30, 1),
        ("the model validation interval", "months", 6, 36, 1),
        ("the model inventory review interval", "months", 3, 24, 1),
        ("the confidence level for the internal model", "%", 950, 999, 10),
    ],
}

DEFINITION_SUBJECTS = {
    "capital": [
        ("the trading book", "positions held with trading intent or to hedge such positions"),
        ("own funds", "the sum of tier one and tier two capital after deductions"),
        (
            "risk weighted assets",
            "exposures multiplied by the risk weight the regulation assigns them",
        ),
        (
            "a large exposure",
            "an exposure to one client that reaches the limit set in this regulation",
        ),
    ],
    "payments": [
        ("a payment institution", "an undertaking authorised to provide payment services"),
        ("safeguarding", "holding client funds apart from the institution's own funds"),
        ("strong customer authentication", "authentication using two independent elements"),
        (
            "an account information service",
            "an online service providing consolidated account information",
        ),
    ],
    "outsourcing": [
        (
            "a critical or important function",
            "a function whose failure would materially impair the firm's operations",
        ),
        (
            "sub outsourcing",
            "an arrangement where a service provider passes a function to another provider",
        ),
        ("an exit plan", "a documented plan for withdrawing from an outsourcing arrangement"),
    ],
    "model-risk": [
        ("an internal model", "a model a firm uses to calculate its own regulatory figures"),
        ("model validation", "an independent assessment of a model's fitness for its stated use"),
        ("backtesting", "comparing a model's predictions against what actually occurred"),
    ],
}

OBLIGATION_SUBJECTS = {
    "capital": [
        "hold own funds at least equal to the requirement set out in this article",
        "report its capital position to the competent authority each quarter",
        "notify the competent authority before it breaches a buffer requirement",
        "document the assumptions behind each exposure classification",
    ],
    "payments": [
        "safeguard client funds in a separate account with a credit institution",
        "report a major operational incident to the competent authority",
        "apply strong customer authentication to each electronic payment",
        "publish the conditions under which a refund is available",
    ],
    "outsourcing": [
        "maintain a register of every outsourcing arrangement",
        "assess the risk of an outsourcing before entering into it",
        "notify the competent authority before outsourcing a critical function",
        "keep an exit plan for each critical outsourcing arrangement",
    ],
    "model-risk": [
        "validate each internal model before it is used for regulatory purposes",
        "keep an inventory of every model in regulatory use",
        "assign a named owner to each internal model",
        "report backtesting exceptions to the competent authority",
    ],
}

PROCEDURE_SUBJECTS = {
    "capital": [
        (
            "classify an exposure",
            [
                "identify the counterparty",
                "assign the risk weight",
                "record the classification and its rationale",
            ],
        ),
        (
            "apply for a buffer waiver",
            [
                "submit the assessment",
                "await the authority's decision",
                "record the outcome in the capital plan",
            ],
        ),
    ],
    "payments": [
        (
            "report an incident",
            [
                "classify its severity",
                "notify within the reporting window",
                "submit the final report",
            ],
        ),
        (
            "authorise a payment",
            [
                "verify the two authentication elements",
                "check the exemption limit",
                "record the authorisation",
            ],
        ),
    ],
    "outsourcing": [
        (
            "enter an outsourcing arrangement",
            [
                "perform the risk assessment",
                "notify the authority",
                "record the arrangement in the register",
            ],
        ),
        (
            "terminate an arrangement",
            ["invoke the exit plan", "transfer the function", "update the register"],
        ),
    ],
    "model-risk": [
        (
            "approve a model",
            [
                "complete validation",
                "obtain sign off from the model owner",
                "record the approval in the inventory",
            ],
        ),
        (
            "investigate a backtesting exception",
            ["reproduce the result", "identify the cause", "record the finding"],
        ),
    ],
}

RELATIONSHIP_SUBJECTS = {
    "capital": [
        ("the capital requirement", "risk weighted assets", "is calculated from"),
        ("the leverage ratio", "tier one capital", "is derived from"),
        ("a large exposure limit", "own funds", "is expressed as a share of"),
    ],
    "payments": [
        ("the safeguarding obligation", "client funds", "applies to"),
        ("an incident report", "the severity classification", "depends on"),
    ],
    "outsourcing": [
        ("the notification duty", "the criticality assessment", "depends on"),
        ("an exit plan", "the outsourcing register entry", "is attached to"),
    ],
    "model-risk": [
        ("model validation", "the model inventory", "records its outcome in"),
        ("a backtesting exception", "the confidence level", "is measured against"),
    ],
}


# ----------------------------------------------------------------- building --


def _numeric(rng: Rng, domain: str) -> tuple[str, dict, list[str]]:
    label, unit, low, high, scale = rng.choice(NUMERIC_SUBJECTS[domain])
    raw = rng.randint(low, high)
    amount = f"{raw / scale:g}" if scale != 1 else str(raw)
    statement = f"{label.capitalize()} is {amount} {unit}."
    return statement, {"amount": amount, "unit": unit, "label": label}, [label]


def _date(rng: Rng, domain: str) -> tuple[str, dict, list[str]]:
    label, _, _, _, _ = rng.choice(NUMERIC_SUBJECTS[domain])
    year = rng.choice([2024, 2025, 2026, 2027])
    month = rng.randint(1, 12)
    day = rng.randint(1, 28)
    date = f"{year}-{month:02d}-{day:02d}"
    statement = f"The requirement covering {label} applies from {date}."
    return statement, {"date": date, "label": label}, [label]


def _definition(rng: Rng, domain: str) -> tuple[str, dict, list[str]]:
    term, meaning = rng.choice(DEFINITION_SUBJECTS[domain])
    statement = f"{term.capitalize()} means {meaning}."
    return statement, {"text": meaning, "term": term, "key_phrases": _key_phrases(meaning)}, [term]


def _key_phrases(meaning: str) -> list[str]:
    """Doc 02 section 11: a definition matches by required key phrases listed in
    the fact, rather than by string equality on a whole sentence."""
    words = [w for w in meaning.split() if len(w) > 4]
    return words[:3]


def _obligation(rng: Rng, domain: str) -> tuple[str, dict, list[str]]:
    duty = rng.choice(OBLIGATION_SUBJECTS[domain])
    statement = f"An institution shall {duty}."
    return statement, {"text": duty, "key_phrases": _key_phrases(duty)}, []


def _procedure(rng: Rng, domain: str) -> tuple[str, dict, list[str]]:
    goal, steps = rng.choice(PROCEDURE_SUBJECTS[domain])
    joined = ", then ".join(steps)
    statement = f"To {goal}, an institution shall {joined}."
    return statement, {"text": joined, "steps": steps, "goal": goal, "key_phrases": steps}, []


def _relationship(rng: Rng, domain: str) -> tuple[str, dict, list[str]]:
    a, b, verb = rng.choice(RELATIONSHIP_SUBJECTS[domain])
    statement = f"{a.capitalize()} {verb} {b}."
    return (
        statement,
        {"text": f"{a} {verb} {b}", "from": a, "to": b, "kind": verb, "key_phrases": [a, b]},
        [a, b],
    )


BUILDERS = {
    "number": _numeric,
    "date": _date,
    "definition": _definition,
    "obligation": _obligation,
    "procedure": _procedure,
    "relationship": _relationship,
}


def _concepts_for(rng: Rng, domain: str) -> list[str]:
    pool = CONCEPTS[domain]
    picked = rng.sample(pool, rng.randint(1, 2))
    return [cid for cid, _ in picked]


def generate_facts(seed: int, total: int = TOTAL_FACTS) -> list[Fact]:
    """The ledger, before anything is planted in a document."""
    rng = Rng(seed, "facts")
    facts: list[Fact] = []

    per_domain = total // len(DOMAINS)
    for domain_index, domain in enumerate(DOMAINS):
        domain_rng = rng.derive(domain)
        for i in range(per_domain):
            kind: FactKind = domain_rng.weighted(KIND_WEIGHTS)
            fact_rng = domain_rng.derive(str(i))
            statement, value, refs = BUILDERS[kind](fact_rng, domain)

            number = domain_index * per_domain + i + 1
            firms = firms_for(domain)
            entity_refs = list(refs)
            if fact_rng.chance(0.35) and firms:
                entity_refs.append(fact_rng.choice(firms).name)

            facts.append(
                Fact(
                    fact_id=f"F-{number:04d}",
                    domain=domain,
                    statement=statement,
                    kind=kind,
                    value=value,
                    entity_refs=entity_refs,
                    truth="true",
                    concept_ids=_concepts_for(fact_rng, domain),
                    # Articles run 1 to 120 per doc 02 section 4.
                    article=1 + (i % 120),
                )
            )

    _add_superseded(seed, facts)
    _add_false_plants(seed, facts)
    return facts


def _bump(value: dict, rng: Rng) -> dict:
    """A v2 value that differs from v1 in a way a matcher can tell apart."""
    out = dict(value)
    if "amount" in out:
        try:
            current = float(out["amount"])
        except ValueError:
            return out
        step = rng.choice([0.5, 1, 1.5, 2, 2.5])
        raised = current + step if rng.chance(0.7) else max(0.5, current - step)
        out["amount"] = f"{raised:g}"
    elif "date" in out:
        year, month, day = (int(p) for p in str(out["date"]).split("-"))
        out["date"] = f"{year + 1}-{month:02d}-{day:02d}"
    elif "text" in out:
        out["text"] = out["text"] + ", as amended"
        out["key_phrases"] = out.get("key_phrases", []) + ["amended"]
    return out


def _add_superseded(seed: int, facts: list[Fact]) -> None:
    """Doc 02 section 5.2: CAR3 v1 and v2 both exist, and 30 facts change value.

    The v1 fact is marked `superseded` and points at its replacement, so a card
    written at T1 that still cites the v1 value is detectably stale at T3.
    """
    rng = Rng(seed, "facts", "superseded")
    # Only the CAR3 domains version, because only CAR3 gets a v2.
    candidates = [
        f
        for f in facts
        if REGULATION_FOR_DOMAIN[f.domain] == "car3" and f.kind in ("number", "date")
    ]
    chosen = rng.sample(candidates, SUPERSEDED_COUNT)

    next_number = max(int(f.fact_id.split("-")[1]) for f in facts) + 1
    for offset, old in enumerate(chosen):
        new_id = f"F-{next_number + offset:04d}"
        new = Fact(
            fact_id=new_id,
            domain=old.domain,
            statement=old.statement,
            kind=old.kind,
            value=_bump(old.value, rng.derive(old.fact_id)),
            entity_refs=list(old.entity_refs),
            truth="true",
            concept_ids=list(old.concept_ids),
            article=old.article,
            supersedes=old.fact_id,
        )
        # Restate the sentence around the new value so the v2 text reads right.
        new.statement = _restate(old.statement, old.display_value, new.display_value)
        old.truth = "superseded"
        old.superseded_by = new_id
        facts.append(new)


def _restate(statement: str, old_value: str, new_value: str) -> str:
    return statement.replace(old_value, new_value) if old_value in statement else statement


def _add_false_plants(seed: int, facts: list[Fact]) -> None:
    """Doc 02 section 3: facts that are wrong on purpose.

    A false plant is a *variant* of a true fact, not a fact about nothing: that
    is what makes it a trap. An answer that cites the press page for the wrong
    threshold is exactly the failure the forbidden fact rate measures.
    """
    rng = Rng(seed, "facts", "false_plants")
    candidates = [f for f in facts if f.truth == "true" and f.kind in ("number", "date")]
    chosen = rng.sample(candidates, FALSE_PLANT_COUNT)

    next_number = max(int(f.fact_id.split("-")[1]) for f in facts) + 1
    for offset, real in enumerate(chosen):
        wrong_value = _bump(real.value, rng.derive(real.fact_id, "wrong"))
        wrong = Fact(
            fact_id=f"F-{next_number + offset:04d}",
            domain=real.domain,
            statement=_restate(real.statement, real.display_value, _display(wrong_value)),
            kind=real.kind,
            value=wrong_value,
            entity_refs=list(real.entity_refs),
            truth="false_plant",
            concept_ids=list(real.concept_ids),
            article=real.article,
            # Points at the fact it misquotes, so a scorer can say which value
            # the answer should have had.
            supersedes=real.fact_id,
        )
        facts.append(wrong)


def _display(value: dict) -> str:
    if "amount" in value:
        return f"{value['amount']} {value.get('unit', '')}".strip()
    if "date" in value:
        return str(value["date"])
    return str(value.get("text", ""))


def summarise(facts: list[Fact]) -> dict:
    """What `gen verify` prints, and what the README records."""
    kinds: dict[str, int] = {}
    domains: dict[str, int] = {}
    truths: dict[str, int] = {}
    for f in facts:
        kinds[f.kind] = kinds.get(f.kind, 0) + 1
        domains[f.domain] = domains.get(f.domain, 0) + 1
        truths[f.truth] = truths.get(f.truth, 0) + 1
    numeric = kinds.get("number", 0) + kinds.get("date", 0)
    return {
        "total": len(facts),
        "by_kind": dict(sorted(kinds.items())),
        "by_domain": dict(sorted(domains.items())),
        "by_truth": dict(sorted(truths.items())),
        "numeric_share": round(numeric / max(len(facts), 1), 3),
    }
