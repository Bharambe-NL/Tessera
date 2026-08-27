"""The learning path and the scripted learners. Doc 17 section 10.

"Synthetic learners: scripted answer policies (always right, right below level
3, random, overconfident rater) run against a synthetic path of 20 concepts."

Two things live here and nothing else does. The **path** is prerequisite ground
truth: twenty concepts and which of them has to be understood before which. The
**learners** are policies, not transcripts: each one says what it would claim
about itself and what it would actually answer, so a run drives the real
pipeline and the score compares what the product decided against what the
policy would have made true.

Everything is derived from the seed, so two builds of one corpus give one path
and one set of learners.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field

from .entities import DOMAINS
from .rng import Rng

#: Doc 17 section 10: "a synthetic path of 20 concepts".
PATH_SIZE = 20

#: Doc 17 section 4's ladder, so a policy can answer per level.
LEVELS = (1, 2, 3, 4)

#: The shape of the tree: how many concepts sit at each prerequisite depth.
#: Four roots, then a widening middle, then the few things that need everything
#: under them. A chain of twenty would make the frontier trivial to guess and a
#: flat twenty would make it meaningless.
DEPTH_SHAPE = (4, 6, 6, 4)


@dataclass
class PathConcept:
    concept_id: str
    term: str
    domain: str
    #: The concepts that have to come first. Ground truth: the Learning Planner
    #: proposes edges and this is what a proposal is scored against.
    prerequisite_ids: list[str] = field(default_factory=list)
    #: Prerequisite depth, counted from zero. Derived from the ids above and
    #: recorded so a scorer does not have to walk the graph to know it.
    depth: int = 0
    level_hint: int = 1


@dataclass
class Learner:
    learner_id: str
    #: always_right | right_below_three | random | overconfident
    policy: str
    #: What the learner claims per concept, doc 17 section 2.1's 0 to 3.
    ratings: dict[str, int] = field(default_factory=dict)
    #: What they can actually answer, per concept, per level. Ground truth the
    #: policy applies and the product never sees.
    answers: dict[str, list[int]] = field(default_factory=dict)
    #: The concepts doc 17 section 3's rule should put the first lesson at,
    #: given the ratings above and no checks yet.
    expected_frontier: list[str] = field(default_factory=list)


@dataclass
class LearningTruth:
    path: list[PathConcept] = field(default_factory=list)
    learners: list[Learner] = field(default_factory=list)

    def to_json(self) -> dict:
        return {
            "path": [asdict(c) for c in self.path],
            "learners": [asdict(learner) for learner in self.learners],
        }


# ------------------------------------------------------------------- terms --

#: Terms per domain, enough for the whole path. Written out rather than drawn
#: from the fact ledger because a concept is a thing a person learns, and the
#: ledger's labels are the values a document states about one.
TERMS = {
    "capital": [
        "capital base",
        "risk weighted assets",
        "capital ratio",
        "capital buffer",
        "buffer stacking",
        "distribution restriction",
    ],
    "payments": [
        "payment account",
        "payment initiation",
        "strong customer authentication",
        "authentication exemption",
        "fraud reporting",
    ],
    "outsourcing": [
        "outsourcing arrangement",
        "criticality assessment",
        "exit plan",
        "subcontracting chain",
        "register of arrangements",
    ],
    "model-risk": [
        "model inventory",
        "model validation",
        "validation interval",
        "model change policy",
    ],
}


def generate(seed: int) -> LearningTruth:
    """One path of twenty concepts and four learners who could walk it."""
    rng = Rng(seed, "learning")
    truth = LearningTruth()

    # ---- the path ---------------------------------------------------------
    # Depth by construction: every concept at depth n names one or two
    # prerequisites at depth n minus one, so the ground truth is a fact about
    # how it was built rather than something computed afterwards and hoped to
    # agree.
    pool = [(domain, term) for domain in DOMAINS for term in TERMS[domain]]
    ordered = rng.shuffled(pool)[:PATH_SIZE]
    by_depth: list[list[PathConcept]] = []
    index = 0
    for depth, width in enumerate(DEPTH_SHAPE):
        level: list[PathConcept] = []
        for _ in range(width):
            if index >= len(ordered):
                break
            domain, term = ordered[index]
            concept = PathConcept(
                concept_id=f"LC-{index + 1:02d}",
                term=term,
                domain=domain,
                depth=depth,
                # Doc 17 section 2.1's hint: the level a path suggests opening
                # at. Deeper ideas are asked about harder.
                level_hint=min(depth + 1, 4),
            )
            if depth > 0 and by_depth[depth - 1]:
                parents = rng.derive("edges", concept.concept_id).sample(
                    [c.concept_id for c in by_depth[depth - 1]], k=1 + (index % 2)
                )
                concept.prerequisite_ids = sorted(parents)
            level.append(concept)
            index += 1
        by_depth.append(level)
        truth.path.extend(level)

    # ---- the learners -----------------------------------------------------
    for policy in ("always_right", "right_below_three", "random", "overconfident"):
        truth.learners.append(_learner(rng.derive(policy), policy, truth.path))

    return truth


def _learner(rng: Rng, policy: str, path: list[PathConcept]) -> Learner:
    learner = Learner(learner_id=policy.replace("_", "-"), policy=policy)

    for concept in path:
        if policy == "overconfident":
            # Doc 17 section 3's case: claims to be able to apply everything and
            # can answer only what sits at the bottom. The whole placement flow
            # exists to catch this within two checks.
            rating = 3
            answers = LEVELS if concept.depth == 0 else ()
        elif policy == "always_right":
            rating = 2 if concept.depth < 2 else 0
            answers = LEVELS
        elif policy == "right_below_three":
            rating = 2 if concept.depth < 2 else 0
            answers = (1, 2)
        else:
            # Deterministic per concept, so "random" is a policy rather than a
            # different corpus every run.
            draw = rng.derive(concept.concept_id)
            rating = draw.randint(0, 3)
            answers = tuple(level for level in LEVELS if draw.derive(str(level)).chance(0.5))

        learner.ratings[concept.concept_id] = rating
        learner.answers[concept.concept_id] = list(answers)

    learner.expected_frontier = _frontier(learner, path)
    return learner


def _frontier(learner: Learner, path: list[PathConcept]) -> list[str]:
    """Doc 17 section 3, before any check has been answered.

    "The lowest prerequisite level where rated concepts have a rating of 2 or
    more and mastery is still unverified." At the start of a placement nothing
    is verified, so the rule reduces to the shallowest depth the learner claims
    to know, and every concept at it.
    """
    claimed = [c for c in path if learner.ratings.get(c.concept_id, 0) >= 2]
    if not claimed:
        return []
    lowest = min(c.depth for c in claimed)
    return sorted(c.concept_id for c in claimed if c.depth == lowest)


def summarise(truth: LearningTruth) -> dict:
    return {
        "concepts": len(truth.path),
        "edges": sum(len(c.prerequisite_ids) for c in truth.path),
        "depths": sorted({c.depth for c in truth.path}),
        "learners": [
            {
                "learner_id": learner.learner_id,
                "policy": learner.policy,
                "claims": sum(1 for r in learner.ratings.values() if r >= 2),
                "frontier": len(learner.expected_frontier),
            }
            for learner in truth.learners
        ],
    }
