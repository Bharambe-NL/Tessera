"""The breadth question set. BN-036.

The owner's directive, verbatim: "The new questions must be varied and not
just stupid finance related questions. we were restricting a bit too much."

These questions test the Router's judgments that are pack independent: does
the question carry regulatory stakes, and how much depth does it deserve. The
finance corpus cannot test the stakes judgment, because every question in it
is consequential by construction, so a model that always answered true would
score perfectly. This set has real negatives.

Every question is hand written rather than templated, because variety is the
point, and it carries explicit ground truth: `regulatory_stakes` and
`depth_expected`. There is no retrieval ground truth here; the corpus knows
nothing about paracetamol or Hamlet, and honest behaviour downstream is a
no-sources card. What is scored is the routing, not the answer.

The bank is fixed and the seed only orders it, so the set is deterministic and
additions are reviewable line by line.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass

from .rng import Rng


@dataclass
class BreadthQuestion:
    q_id: str
    text: str
    #: The field the question lives in. Informational: no pack governs these,
    #: so the observed domain label is expected to stay `unknown`.
    domain: str
    #: Ground truth for the Router's stakes judgment.
    regulatory_stakes: bool
    depth_expected: str
    snapshot: str = "T1"

    def to_json(self) -> dict:
        out = asdict(self)
        # The shape the eval runner reads, doc 02 section 6. Empty ground truth
        # for everything retrieval scores, because nothing here is in the
        # corpus and a no-sources card is the honest outcome.
        out.update(
            audience_id=None,
            required_facts=[],
            required_sources=[],
            forbidden_facts=[],
            expected_visual="none",
            expected_flags=[],
            edge_case_ids=["breadth"],
            parent_q_id=None,
            anchor_text=None,
        )
        return out


#: (text, field, stakes, depth). Stakes true means the answer turns on a rule,
#: threshold, date or obligation the reader might act on.
BANK: tuple[tuple[str, str, bool, str], ...] = (
    # ---- consequential, spread across fields ------------------------------
    ("What is the maximum daily dose of paracetamol for an adult?", "health", True, "deep"),
    ("How long after a tick bite should someone watch for symptoms?", "health", True, "deep"),
    ("At what internal temperature is chicken safe to eat?", "food-safety", True, "deep"),
    ("How long can cooked rice be kept at room temperature?", "food-safety", True, "deep"),
    ("What notice period ends a residential lease in Germany?", "housing", True, "deep"),
    ("Can a landlord raise the rent twice in one year?", "housing", True, "deep"),
    ("How many days can a tourist stay in Schengen without a visa?", "travel", True, "deep"),
    ("Do I need to declare cash over 10,000 euros when entering the EU?", "travel", True, "deep"),
    ("What is the alcohol limit for driving in the Netherlands?", "driving", True, "deep"),
    ("When must winter tyres be fitted in Austria?", "driving", True, "deep"),
    ("How long does an employer have to issue a payslip after payday?", "employment", True, "deep"),
    ("Is a verbal job offer binding before the contract is signed?", "employment", True, "deep"),
    ("What is the deadline for filing a corrected VAT return?", "tax", True, "deep"),
    ("Are crypto gains taxable if the coins were never converted to euros?", "tax", True, "deep"),
    ("How long may a shop take to refund a returned online order?", "consumer", True, "deep"),
    ("Does a two year warranty cover a battery that degrades?", "consumer", True, "deep"),
    ("When does a hobby drone need to be registered?", "aviation", True, "deep"),
    ("Can an airline deny boarding over passport validity?", "aviation", True, "deep"),
    ("How long can a company keep CCTV footage of employees?", "privacy", True, "deep"),
    ("Does a website need consent to set analytics cookies?", "privacy", True, "deep"),
    ("Is planning permission needed for a garden office?", "construction", True, "deep"),
    ("What insulation standard applies to a new roof?", "construction", True, "deep"),
    ("Can a school share a pupil's grades with a stepparent?", "education", True, "deep"),
    ("What vaccinations are mandatory for school enrolment?", "education", True, "deep"),
    ("Is it legal to record a call without consent in California?", "communications", True, "deep"),
    ("How long must a clinical trial keep participant records?", "research", True, "research"),
    ("What are the rules on lithium batteries in checked luggage?", "travel", True, "deep"),
    ("When is a food product allowed to be labelled organic?", "food-safety", True, "research"),
    ("What rest breaks must a lorry driver take on a long shift?", "employment", True, "deep"),
    ("Under what conditions may a minor open a bank account alone?", "consumer", True, "deep"),
    # ---- casual, spread across fields --------------------------------------
    ("Why is the sky blue?", "science", False, "fast"),
    ("How does a jet engine produce thrust?", "engineering", False, "fast"),
    ("Who composed the Goldberg Variations?", "music", False, "fast"),
    ("What is the difference between TCP and UDP?", "technology", False, "fast"),
    ("How does a sourdough starter work?", "cooking", False, "fast"),
    ("Why did the Western Roman Empire fall?", "history", False, "deep"),
    ("What makes a good opening in chess?", "games", False, "fast"),
    ("How are glaciers formed?", "geography", False, "fast"),
    ("What is the plot of Hamlet?", "literature", False, "fast"),
    ("How does photosynthesis store energy?", "biology", False, "fast"),
    ("Why do cats purr?", "animals", False, "fast"),
    ("What is the difference between espresso and filter coffee?", "cooking", False, "fast"),
    ("How do noise cancelling headphones work?", "technology", False, "fast"),
    ("Who painted the ceiling of the Sistine Chapel?", "art", False, "fast"),
    ("Why is the ocean salty?", "science", False, "fast"),
    ("What language family does Hungarian belong to?", "linguistics", False, "fast"),
    ("How did the printing press change Europe?", "history", False, "research"),
    ("What is the offside rule in football?", "sport", False, "fast"),
    ("How does a compiler differ from an interpreter?", "technology", False, "fast"),
    ("Why do onions make you cry?", "cooking", False, "fast"),
    ("What are the movements of a classical symphony?", "music", False, "fast"),
    ("How do bees communicate the location of flowers?", "biology", False, "fast"),
    ("What caused the 1815 year without a summer?", "history", False, "deep"),
    ("How does public key cryptography work in principle?", "technology", False, "deep"),
    ("Why are flamingos pink?", "animals", False, "fast"),
    ("What is the Ship of Theseus problem?", "philosophy", False, "fast"),
    ("How was the age of the universe estimated?", "science", False, "deep"),
    ("What distinguishes a sonnet from other poems?", "literature", False, "fast"),
    ("Why does bread go stale faster in the fridge?", "cooking", False, "fast"),
    ("How did trade routes shape the spread of the Black Death?", "history", False, "research"),
)


def generate(seed: int) -> list[BreadthQuestion]:
    """The bank in a seed dependent order, numbered after shuffling."""
    rng = Rng(seed, "breadth")
    rows = rng.shuffled(BANK)
    return [
        BreadthQuestion(
            q_id=f"B-{i + 1:04d}",
            text=text,
            domain=field,
            regulatory_stakes=stakes,
            depth_expected=depth,
        )
        for i, (text, field, stakes, depth) in enumerate(rows)
    ]


def summarise(questions: list[BreadthQuestion]) -> dict:
    return {
        "total": len(questions),
        "with_stakes": sum(1 for q in questions if q.regulatory_stakes),
        "without_stakes": sum(1 for q in questions if not q.regulatory_stakes),
        "fields": len({q.domain for q in questions}),
    }
