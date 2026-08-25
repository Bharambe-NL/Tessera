"""Layer 4, time evolution. Doc 02 section 5.4.

"The corpus is generated as a sequence of snapshots at T0, T1, T2, T3 (three
month steps)."

A board created at T1 and reopened at T3 should show `source.stale` on the
affected citations, and the Verifier should flag cards whose values changed. The
harness runs the same question set at each snapshot and compares (doc 02 section
10.2's staleness detection, threshold 0.95).

A snapshot is a *view*: which files exist at that time, and what each one hashes
to. The documents themselves are generated once, so the same document at two
snapshots is the same bytes unless something deliberately changed it.
"""

from __future__ import annotations

import hashlib
from dataclasses import asdict, dataclass, field

from .corpus import Document
from .rng import Rng

#: Doc 02 section 5.4: three month steps.
TIMELINE = {
    "T0": "2025-04-01",
    "T1": "2025-07-01",
    "T2": "2025-10-01",
    "T3": "2026-01-01",
}

#: CAR3 v2 is published at T2 and applies from T3.
CAR3_V2_PUBLISHED = "T2"
CAR3_V2_APPLIES = "T3"


@dataclass
class FileState:
    doc_id: str
    path: str
    content_hash: str
    #: Why this file looks different from the last snapshot, if it does.
    change: str | None = None


@dataclass
class Snapshot:
    label: str
    at: str
    files: list[FileState] = field(default_factory=list)
    #: Facts that are authoritative at this snapshot. A card written here and
    #: read later is scored against these.
    facts_in_force: list[str] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    def to_json(self) -> dict:
        out = asdict(self)
        out["files"] = [asdict(f) for f in self.files]
        return out


def content_hash(doc: Document) -> str:
    return hashlib.sha256(doc.body.encode("utf-8")).hexdigest()


def build(seed: int, documents: list[Document], facts) -> list[Snapshot]:
    """The four snapshots, each a list of the files that exist at that time."""
    rng = Rng(seed, "snapshots")

    car3_v1 = "reg-car3-v1"
    car3_v2 = "reg-car3-v2"

    # Documents added, revised and deleted between snapshots. Chosen once, from
    # internal documents only, so a regulation never quietly disappears.
    internal = [d.doc_id for d in documents if d.kind == "internal" and "broken" not in d.doc_id]
    web = [d.doc_id for d in documents if d.kind == "web"]

    added_at_t1 = set(rng.derive("added_t1").sample(internal, 3))
    added_at_t2 = set(
        rng.derive("added_t2").sample([d for d in internal if d not in added_at_t1], 3)
    )
    revised_at_t2 = set(
        rng.derive("revised_t2").sample(
            [d for d in internal if d not in added_at_t1 | added_at_t2], 4
        )
    )
    deleted_at_t3 = set(
        rng.derive("deleted_t3").sample(
            [d for d in internal if d not in added_at_t1 | added_at_t2 | revised_at_t2], 2
        )
    )

    # Doc 02 section 5.4: two web pages are taken down at T3, so their locators
    # stop resolving. Doc 05 section 7 emits source.stale with locator_gone.
    taken_down_at_t3 = set(rng.derive("taken_down").sample(web, 2))

    # Doc 02 open question 2, resolved as proposed: two pages change content at
    # T2 while keeping their locator, to test content_hash on re-verification.
    silent_edits = {d.doc_id for d in documents if d.edge_case_id == "silent_edit"}

    superseded_ids = {f.fact_id for f in facts if f.truth == "superseded"}
    v2_ids = {f.fact_id for f in facts if f.supersedes and f.truth == "true"}
    stable_ids = {f.fact_id for f in facts if f.truth == "true" and f.fact_id not in v2_ids}

    snapshots: list[Snapshot] = []
    for label, at in TIMELINE.items():
        snap = Snapshot(label=label, at=at)

        for doc in documents:
            # CAR3 v2 does not exist before it is published.
            if doc.doc_id == car3_v2 and _before(label, CAR3_V2_PUBLISHED):
                continue
            if doc.doc_id in added_at_t1 and _before(label, "T1"):
                continue
            if doc.doc_id in added_at_t2 and _before(label, "T2"):
                continue
            if doc.doc_id in deleted_at_t3 and not _before(label, "T3"):
                continue
            if doc.doc_id in taken_down_at_t3 and not _before(label, "T3"):
                continue

            digest = content_hash(doc)
            change = None
            if doc.doc_id in revised_at_t2 and not _before(label, "T2"):
                # A revision that keeps the locator: the hash moves, the path
                # does not, which is what a re-verification has to notice.
                digest = hashlib.sha256(
                    (doc.body + "\n\nRevised at T2.").encode("utf-8")
                ).hexdigest()
                change = "revised"
            if doc.doc_id in silent_edits and not _before(label, "T2"):
                digest = hashlib.sha256(
                    (doc.body + "\n\nSilently edited.").encode("utf-8")
                ).hexdigest()
                change = "silent_edit"
            if doc.doc_id in added_at_t1 and label == "T1":
                change = "added"
            if doc.doc_id in added_at_t2 and label == "T2":
                change = "added"

            snap.files.append(FileState(doc.doc_id, doc.path, digest, change))

        # Which facts are authoritative here. Before CAR3 v2 applies, the v1
        # values stand; from T3 the v2 values do, and a card citing v1 is stale.
        if _before(label, CAR3_V2_APPLIES):
            snap.facts_in_force = sorted(stable_ids | superseded_ids)
        else:
            snap.facts_in_force = sorted(stable_ids | v2_ids)

        if label == CAR3_V2_PUBLISHED:
            snap.notes.append(f"{car3_v2} is published but does not apply until {CAR3_V2_APPLIES}.")
        if label == CAR3_V2_APPLIES:
            snap.notes.append(
                f"{car3_v2} applies. A card written before this that cites {car3_v1} for a "
                "changed value is stale."
            )
            snap.notes.append(f"{len(taken_down_at_t3)} web pages no longer resolve.")

        snapshots.append(snap)

    return snapshots


def _before(label: str, other: str) -> bool:
    order = list(TIMELINE)
    return order.index(label) < order.index(other)


def stale_at_t3(facts) -> list[str]:
    """The facts whose value changed, which is what staleness detection is
    scored against (doc 02 section 10.2)."""
    return sorted(f.fact_id for f in facts if f.truth == "superseded")
