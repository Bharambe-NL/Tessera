"""Layer 1, the baseline corpus. Doc 02 section 5.1.

"Clean documents that state facts once, clearly, with no contradictions. Layer 1
output is what a well behaved fast or deep answer should find. Retrieval recall
is measured here."

Prose is written from templates rather than by a model. Doc 02 section 5.1 has a
model write the paragraphs and a deterministic pass confirm every planted value
appears verbatim; doc 02 section 9 requires the ledger to be identical at the same
seed either way. Templates make `gen build --seed 42` free, offline and byte
identical, which is what CI needs, and the deterministic verification pass is the
part that actually guarantees the corpus is usable. A model backend can be added
behind a flag without moving a single fact (BN-017).
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field

from .entities import (
    DOMAIN_FOLDER,
    REGULATION_FOR_DOMAIN,
    REGULATIONS,
    by_id,
    firms_for,
    sites_for,
    web_domain,
)
from .facts import Fact, Planting
from .rng import Rng


@dataclass
class Passage:
    passage_id: str
    text: str
    location: dict = field(default_factory=dict)
    #: Facts this passage carries, and how faithfully.
    plants: list[tuple[str, str]] = field(default_factory=list)


@dataclass
class Document:
    doc_id: str
    kind: str  # regulatory | internal | web
    title: str
    path: str
    format: str  # md | html | docx | xlsx | pdf | txt
    passages: list[Passage] = field(default_factory=list)
    issuer: str | None = None
    published_at: str | None = None
    version_ref: str | None = None
    #: Doc 02 section 5.2: so the harness can score by case.
    edge_case_id: str | None = None
    language: str = "en"
    #: Set by layer 3, so a failure traces to the transformation that caused it.
    transformations: list[str] = field(default_factory=list)

    @property
    def body(self) -> str:
        return "\n\n".join(p.text for p in self.passages)

    def to_json(self) -> dict:
        out = asdict(self)
        out["passages"] = [
            {
                "passage_id": p.passage_id,
                "location": p.location,
                "plants": [{"fact_id": f, "fidelity": fd} for f, fd in p.plants],
            }
            for p in self.passages
        ]
        return out


# ---------------------------------------------------------------- phrasing --


def state_exact(fact: Fact) -> str:
    """The passage states the fact verbatim. Fidelity `exact`."""
    return fact.statement


def state_paraphrase(rng: Rng, fact: Fact) -> str:
    """Same meaning, different words. The value itself is never altered: a
    paraphrase that moved the number would be a contradiction, not a paraphrase."""
    value = fact.display_value
    label = fact.value.get("label") or fact.value.get("term") or fact.value.get("goal")

    if fact.kind == "number" and label:
        return rng.choice(
            [
                f"For {label}, the figure that applies is {value}.",
                f"Firms should read {label} as {value}.",
                f"In practice {label} comes to {value}.",
            ]
        )
    if fact.kind == "date" and label:
        return rng.choice(
            [
                f"The rule on {label} takes effect on {value}.",
                f"From {value} onward, the requirement covering {label} is in force.",
            ]
        )
    if fact.kind == "definition":
        term = fact.value.get("term", "the term")
        return f"What counts as {term} is {fact.value.get('text', '')}."
    if fact.kind == "obligation":
        return f"Firms are expected to {fact.value.get('text', '')}."
    if fact.kind == "procedure":
        return f"The sequence runs: {fact.value.get('text', '')}."
    return fact.statement


def state_partial(rng: Rng, fact: Fact) -> str:
    """The passage gives part of the value. Doc 02 section 5.2's partial values
    case: "a page gives 'around 8 percent' where the regulation says '8 percent
    of RWA, with a 2.5 percent buffer'."."""
    label = fact.value.get("label") or fact.value.get("term") or "the requirement"
    if fact.kind == "number":
        amount = fact.value.get("amount", "")
        return rng.choice(
            [
                f"{label.capitalize()} sits at around {amount} percent, "
                "depending on how it is read.",
                f"Broadly, {label} is about {amount}, though the text is more precise.",
            ]
        )
    if fact.kind == "date":
        year = str(fact.value.get("date", ""))[:4]
        return f"The requirement covering {label} lands sometime in {year}."
    text = str(fact.value.get("text", ""))
    half = text.split(",")[0]
    return f"{label.capitalize()} involves {half}."


def state_contradiction(rng: Rng, fact: Fact, wrong: str) -> str:
    """The passage states a different value. Doc 02 section 5.2 uses these to
    test source hierarchy and freshness."""
    label = fact.value.get("label") or "the requirement"
    return rng.choice(
        [
            f"{label.capitalize()} is {wrong}.",
            f"Reporting suggests {label} has been set at {wrong}.",
        ]
    )


# --------------------------------------------------------------- regulatory --

ARTICLE_OPENERS = (
    "This Article applies to institutions authorised under this Regulation.",
    "For the purposes of this Article, the following applies.",
    "Competent authorities shall supervise compliance with this Article.",
    "This Article is without prejudice to the requirements in the preceding Chapter.",
)


def build_regulation(
    seed: int,
    regulation_id: str,
    facts: list[Fact],
    version_ref: str | None = None,
    published_at: str = "2024-01-15",
) -> Document:
    """One consolidated regulation text. Doc 02 section 5.1: 60 to 120 articles,
    each a heading and one to four paragraphs, each paragraph planting one or two
    facts at `exact` fidelity."""
    reg = by_id(regulation_id)
    suffix = f"-{version_ref}" if version_ref else ""
    doc_id = f"reg-{regulation_id}{suffix}"
    rng = Rng(seed, "corpus", "regulatory", doc_id)

    title = reg.name if not version_ref else f"{reg.name} ({version_ref})"
    doc = Document(
        doc_id=doc_id,
        kind="regulatory",
        title=title,
        path=f"regulatory/{doc_id}.md",
        format="md",
        issuer=reg.issuer,
        published_at=published_at,
        version_ref=version_ref,
    )

    by_article: dict[int, list[Fact]] = {}
    for f in facts:
        by_article.setdefault(f.article or 1, []).append(f)

    header = Passage(
        passage_id=f"{doc_id}-p0000",
        text=f"# {title}\n\nIssued by the {reg.issuer}.\n\n{reg.description}",
        location={"section": "preamble"},
    )
    doc.passages.append(header)

    counter = 1
    for article in sorted(by_article):
        article_facts = by_article[article]
        heading = Passage(
            passage_id=f"{doc_id}-p{counter:04d}",
            text=f"## Article {article}\n\n{rng.derive(str(article)).choice(ARTICLE_OPENERS)}",
            location={"article": article, "paragraph": 0},
        )
        doc.passages.append(heading)
        counter += 1

        # One or two facts per paragraph, at exact fidelity.
        paragraph = 1
        for chunk_start in range(0, len(article_facts), 2):
            chunk = article_facts[chunk_start : chunk_start + 2]
            sentences = [state_exact(f) for f in chunk]
            body = f"{paragraph}. " + " ".join(sentences)
            passage = Passage(
                passage_id=f"{doc_id}-p{counter:04d}",
                text=body,
                location={"article": article, "paragraph": paragraph},
                plants=[(f.fact_id, "exact") for f in chunk],
            )
            doc.passages.append(passage)
            counter += 1
            paragraph += 1

    return doc


# ----------------------------------------------------------------- internal --

INTERNAL_TYPES = (
    ("policy", "Policy", "md"),
    ("risk-memo", "Risk memo", "md"),
    ("product-spec", "Product specification", "md"),
    ("architecture-note", "Architecture note", "md"),
    ("minutes", "Meeting minutes", "md"),
    ("policy-pdf", "Policy", "pdf"),
    ("board-pack", "Board pack", "docx"),
    ("exposures", "Exposure schedule", "xlsx"),
)


def build_internal(
    seed: int,
    domain: str,
    facts: list[Fact],
    index: int,
    folder: str | None = None,
    language: str = "en",
) -> Document:
    """One internal document. Doc 02 section 5.1: twelve per domain of mixed
    type, each citing the regulation by article number and planting two to six
    facts, at least one at `paraphrase` fidelity."""
    rng = Rng(seed, "corpus", "internal", domain, str(index))
    type_id, type_label, fmt = rng.choice(INTERNAL_TYPES)
    firm = rng.choice(firms_for(domain) or firms_for("capital"))
    folder = folder or DOMAIN_FOLDER[domain]

    doc_id = f"int-{domain}-{index:02d}"
    reg = by_id(REGULATION_FOR_DOMAIN[domain])
    title = f"{type_label}: {domain.replace('-', ' ')} under {reg.name}"

    doc = Document(
        doc_id=doc_id,
        kind="internal",
        title=title,
        path=f"internal/{folder}/{doc_id}.{fmt}",
        format=fmt,
        issuer=firm.name,
        published_at=f"2025-{(index % 12) + 1:02d}-10",
        language=language,
    )

    doc.passages.append(
        Passage(
            passage_id=f"{doc_id}-p0000",
            text=(
                f"# {title}\n\n"
                f"Owner: {firm.name}. This note records how we read {reg.name} "
                f"and what we do about it."
            ),
            location={"section": "intro"},
        )
    )

    # At least one paraphrase, per doc 02 section 5.1.
    # One paraphrase, then a mix. Built to match `facts` exactly so the zip
    # below can be strict.
    fidelities = ["paraphrase"] + [
        rng.choice(["exact", "paraphrase", "partial"]) for _ in range(len(facts) - 1)
    ]

    for i, (fact, fidelity) in enumerate(zip(facts, fidelities, strict=True), start=1):
        p_rng = rng.derive(fact.fact_id)
        if fidelity == "exact":
            body = state_exact(fact)
        elif fidelity == "partial":
            body = state_partial(p_rng, fact)
        else:
            body = state_paraphrase(p_rng, fact)

        text = (
            f"## Section {i}\n\n"
            f"{reg.name} Article {fact.article} is the relevant provision. {body} "
            f"Our position is unchanged since the last review."
        )
        if language == "nl":
            text = _dutch(text, body)

        doc.passages.append(
            Passage(
                passage_id=f"{doc_id}-p{i:04d}",
                text=text,
                location={"section": i, "article_ref": fact.article},
                plants=[(fact.fact_id, fidelity)],
            )
        )

    return doc


def _dutch(text: str, body: str) -> str:
    """Doc 02 section 5.3: 10 percent of the internal folder is Dutch, with the
    fact ledger recording the language.

    The planted value stays in its original form, because a value that changed
    with the language would be untestable: the point of the case is that
    retrieval and citation still work across languages, not translation."""
    header = text.split("\n\n")[0].replace("## Section", "## Onderdeel")
    return (
        f"{header}\n\n"
        f"De betreffende bepaling is het genoemde artikel. {body} "
        f"Ons standpunt is sinds de laatste toetsing ongewijzigd."
    )


# ---------------------------------------------------------------------- web --


def build_web(seed: int, domain: str, facts: list[Fact], index: int) -> Document:
    """One web page. Doc 02 section 5.1: eight per domain, summarising regulation
    for a general reader, planting facts at `paraphrase` and `partial`."""
    rng = Rng(seed, "corpus", "web", domain, str(index))
    site = rng.choice(sites_for(domain) or sites_for("capital"))
    doc_id = f"web-{domain}-{index:02d}"
    reg = by_id(REGULATION_FOR_DOMAIN[domain])

    doc = Document(
        doc_id=doc_id,
        kind="web",
        title=f"What {reg.name} means for you",
        path=f"web/{web_domain(site)}/{doc_id}.html",
        format="html",
        issuer=web_domain(site),
        published_at=f"2025-{(index % 12) + 1:02d}-22",
    )

    doc.passages.append(
        Passage(
            passage_id=f"{doc_id}-p0000",
            text=(
                f"<h1>What {reg.name} means for you</h1>\n"
                f"<p>Published by {site.name}. A plain reading of the rules, without the "
                f"article numbers.</p>"
            ),
            location={"section": "intro"},
        )
    )

    for i, fact in enumerate(facts, start=1):
        p_rng = rng.derive(fact.fact_id)
        fidelity = "partial" if p_rng.chance(0.4) else "paraphrase"
        body = (
            state_partial(p_rng, fact) if fidelity == "partial" else state_paraphrase(p_rng, fact)
        )
        doc.passages.append(
            Passage(
                passage_id=f"{doc_id}-p{i:04d}",
                text=f"<h2>Point {i}</h2>\n<p>{body}</p>",
                location={"section": i},
                plants=[(fact.fact_id, fidelity)],
            )
        )

    return doc


# ------------------------------------------------------------------ linking --


def record_plantings(documents: list[Document], facts: list[Fact]) -> None:
    """Write each document's plantings back onto the facts.

    Doc 02 section 3 keeps `planted_in` on the fact, because that is the
    direction every scorer reads: given a required fact, which documents could
    have supported it."""
    by_id_map = {f.fact_id: f for f in facts}
    for f in facts:
        f.planted_in = []
    for doc in documents:
        for passage in doc.passages:
            for fact_id, fidelity in passage.plants:
                fact = by_id_map.get(fact_id)
                if fact is None:
                    continue
                fact.planted_in.append(
                    Planting(doc_id=doc.doc_id, passage_id=passage.passage_id, fidelity=fidelity)
                )


def verify_exact_plantings(documents: list[Document], facts: list[Fact]) -> list[str]:
    """The deterministic pass from doc 02 section 5.1: confirm every planted
    value really is stated where the fidelity claims it is.

    The test uses `matchers`, not string equality, and that matters. Doc 02
    section 11 defines what counts as stating a fact: numeric equality with unit
    normalisation, date equality across formats, definitions by required key
    phrases. If the generator asserted something stricter than the harness will
    score, it would reject corpora the harness would have read perfectly well.
    The two have to agree, so they share one rule.

    Documents that layer 3 deliberately damaged are exempt. OCR noise exists to
    make values unreadable (doc 02 section 5.3), and doc 02 section 10.3 reports
    scanned recall without a threshold for exactly that reason.
    """
    from . import matchers

    by_id_map = {f.fact_id: f for f in facts}
    damaging = {"ocr_noise", "scanned_no_text_layer", "corrupt", "empty", "password_protected"}
    problems: list[str] = []

    for doc in documents:
        if damaging & set(doc.transformations):
            continue
        for passage in doc.passages:
            for fact_id, fidelity in passage.plants:
                if fidelity != "exact":
                    continue
                fact = by_id_map.get(fact_id)
                if fact is None:
                    problems.append(f"{doc.doc_id}/{passage.passage_id} plants unknown {fact_id}")
                    continue
                if not matchers.matches(fact.kind, fact.value, passage.text):
                    problems.append(
                        f"{doc.doc_id}/{passage.passage_id} claims exact fidelity for "
                        f"{fact_id} ({fact.kind}) but does not state `{fact.display_value}`"
                    )
    return problems


def build_layer_one(seed: int, facts: list[Fact]) -> list[Document]:
    """Every baseline document, in a stable order."""
    from .entities import DOMAINS

    documents: list[Document] = []
    by_domain: dict[str, list[Fact]] = {d: [] for d in DOMAINS}
    for f in facts:
        if f.truth == "true":
            by_domain[f.domain].append(f)

    # Regulatory: one consolidated text per regulation, carrying the true facts
    # of every domain it governs.
    for reg in REGULATIONS:
        reg_facts = [
            f
            for f in facts
            if REGULATION_FOR_DOMAIN[f.domain] == reg.id
            and f.truth == "true"
            and f.supersedes is None
        ]
        version = "v1" if reg.id == "car3" else None
        documents.append(build_regulation(seed, reg.id, reg_facts, version_ref=version))

    # Internal: twelve per domain. Every twelfth lands in Sensitive, and one in
    # ten is Dutch (doc 02 section 5.3).
    for domain in DOMAINS:
        pool = by_domain[domain]
        rng = Rng(seed, "corpus", "internal-plan", domain)
        for index in range(12):
            count = rng.randint(2, 6)
            start = (index * 7) % max(len(pool) - count, 1)
            chosen = pool[start : start + count] or pool[:count]
            folder = "Sensitive" if index == 11 else None
            language = "nl" if index in (3,) else "en"
            documents.append(
                build_internal(seed, domain, chosen, index, folder=folder, language=language)
            )

    # Web: eight per domain.
    for domain in DOMAINS:
        pool = by_domain[domain]
        rng = Rng(seed, "corpus", "web-plan", domain)
        for index in range(8):
            count = rng.randint(1, 4)
            start = (index * 5) % max(len(pool) - count, 1)
            chosen = pool[start : start + count] or pool[:count]
            documents.append(build_web(seed, domain, chosen, index))

    return documents
