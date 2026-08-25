"""The synthetic entities. Doc 02 section 4.

"The corpus needs names that are obviously invented and stable across runs, so a
real regulator or bank never appears in evaluation output."

That constraint is load bearing rather than decorative. Evaluation output gets
read, quoted and pasted; a corpus built on real issuers would eventually produce
a plausible looking claim about a real regulator that nobody planted and nobody
checked. Every name here is invented, and `entities.json` is a deliverable so the
`finance-eu-synthetic` pack can rank them.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field

#: The four domains doc 02 section 3 spreads 600 facts across.
DOMAINS = ("capital", "payments", "outsourcing", "model-risk")


@dataclass(frozen=True)
class Entity:
    id: str
    type: str
    name: str
    description: str
    #: For a regulation, the issuer that publishes it.
    issuer: str | None = None
    #: Domains this entity belongs to.
    domains: tuple[str, ...] = field(default_factory=tuple)


REGULATORS = (
    Entity(
        "capo",
        "regulator",
        "Central Authority for Prudential Oversight",
        "The prudential supervisor. Publishes CAR3 and the outsourcing guidelines.",
        domains=("capital", "outsourcing", "model-risk"),
    ),
    Entity(
        "pcb",
        "regulator",
        "Payments Conduct Board",
        "The conduct supervisor for payment services. Publishes PSD-S.",
        domains=("payments",),
    ),
)

REGULATIONS = (
    Entity(
        "car3",
        "regulation",
        "Capital Adequacy Regulation 3",
        "Capital requirements, buffers and the treatment of exposures. Articles 1 to 120.",
        issuer="Central Authority for Prudential Oversight",
        domains=("capital", "model-risk"),
    ),
    Entity(
        "psd-s",
        "regulation",
        "Payment Services Directive",
        "Authorisation, safeguarding and incident reporting for payment institutions.",
        issuer="Payments Conduct Board",
        domains=("payments",),
    ),
    Entity(
        "og-2025",
        "regulation",
        "Outsourcing Guidelines 2025",
        "Register, risk assessment and exit planning for outsourced functions.",
        issuer="Central Authority for Prudential Oversight",
        domains=("outsourcing",),
    ),
)

FIRMS = (
    Entity(
        "meerkant",
        "firm",
        "Meerkant Bank",
        "A universal bank with a trading book and a retail lending arm.",
        domains=("capital", "model-risk", "outsourcing"),
    ),
    Entity(
        "delta",
        "firm",
        "Delta Payments NV",
        "A payment institution handling card acquiring and account information services.",
        domains=("payments", "outsourcing"),
    ),
    Entity(
        "kaspar",
        "firm",
        "Kaspar Asset Management",
        "An asset manager running internal models for market risk.",
        domains=("model-risk", "outsourcing"),
    ),
)

#: Doc 02 section 4: a fictional trade press site, a fictional consultancy blog,
#: two fictional vendor pages.
WEB_SITES = (
    Entity(
        "ledgerline",
        "web_site",
        "Ledgerline",
        "A trade press site covering supervision.",
        domains=DOMAINS,
    ),
    Entity(
        "northbank-advisory",
        "web_site",
        "Northbank Advisory",
        "A consultancy blog that summarises regulation for a general reader.",
        domains=DOMAINS,
    ),
    Entity(
        "vaultworks",
        "web_site",
        "Vaultworks",
        "A vendor page for a reporting product.",
        domains=("capital", "payments"),
    ),
    Entity(
        "clearpath-systems",
        "web_site",
        "Clearpath Systems",
        "A vendor page for an outsourcing register tool.",
        domains=("outsourcing",),
    ),
)

#: Doc 02 section 5.3 plants facts in a `Sensitive` subfolder that the retriever
#: config excludes by default. Doc 05 section 12 requires 1.00 compliance.
INTERNAL_FOLDERS = (
    "Policies",
    "Risk",
    "Product",
    "Architecture",
    "Minutes",
    "Sensitive",
)

DOMAIN_FOLDER = {
    "capital": "Risk",
    "payments": "Product",
    "outsourcing": "Policies",
    "model-risk": "Risk",
}

#: The domain each regulation governs, so a fact knows which text carries it.
REGULATION_FOR_DOMAIN = {
    "capital": "car3",
    "model-risk": "car3",
    "payments": "psd-s",
    "outsourcing": "og-2025",
}

ALL: tuple[Entity, ...] = REGULATORS + REGULATIONS + FIRMS + WEB_SITES


def by_id(entity_id: str) -> Entity:
    for e in ALL:
        if e.id == entity_id:
            return e
    raise KeyError(f"no synthetic entity `{entity_id}`")


def firms_for(domain: str) -> tuple[Entity, ...]:
    return tuple(f for f in FIRMS if domain in f.domains)


def sites_for(domain: str) -> tuple[Entity, ...]:
    return tuple(s for s in WEB_SITES if domain in s.domains)


def web_domain(site: Entity) -> str:
    """A fake domain per site. `.invalid` is reserved by RFC 2606, so nothing
    here can ever resolve to a real host even if a fetch escapes the harness."""
    return f"{site.id}.invalid"


def manifest() -> dict:
    """`entities.json`, a deliverable per doc 02 section 4."""
    return {
        "generator_note": (
            "Every name here is invented. The corpus exists so evaluation output can be "
            "quoted without a real regulator or bank appearing in it."
        ),
        "domains": list(DOMAINS),
        "internal_folders": list(INTERNAL_FOLDERS),
        "entities": [asdict(e) for e in ALL],
    }


def synthetic_source_hierarchy() -> list[dict]:
    """The `finance-eu-synthetic` pack's source hierarchy: the same rules as
    `finance-eu` with the synthetic issuers substituted in (doc 02 section 4)."""
    return [
        {"class": "regulatory", "issuer_pattern": r.issuer or r.name, "trust_rank": 1}
        for r in REGULATIONS
    ] + [
        {"class": "regulatory", "trust_rank": 2},
        {"class": "structured_query", "trust_rank": 3},
        {"class": "local_document", "trust_rank": 4},
        {"class": "web", "issuer_pattern": "ledgerline.invalid", "trust_rank": 6},
        {"class": "web", "trust_rank": 7},
        {"class": "user_supplied", "trust_rank": 8},
    ]
