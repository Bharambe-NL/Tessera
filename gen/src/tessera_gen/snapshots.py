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

`materialise` turns a view into a tree. It returns the documents as they stand
at one label, so `gen build --snapshot T3` can write a corpus a retriever reads
at T3 rather than a manifest describing one. `build` hashes what `materialise`
returns, so a file's bytes and its manifest entry cannot disagree.
"""

from __future__ import annotations

import copy
import hashlib
from dataclasses import asdict, dataclass, field

from .corpus import Document, Passage
from .edge_cases import SOLE_SOURCE_DOC_ID
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

#: What a revision and a silent edit add to a document body. A snapshot manifest
#: hashes the body these produce, so they are named once and read from here by
#: both the manifest and the tree.
REVISION_NOTE = "Revised at T2."
SILENT_EDIT_NOTE = "Silently edited."

CAR3_V1 = "reg-car3-v1"
CAR3_V2 = "reg-car3-v2"


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


@dataclass(frozen=True)
class SnapshotPlan:
    """Which documents change, and when. Doc 02 section 5.4.

    Drawn once and read by both the manifests and the materialised trees, so the
    two describe one timeline. Every field holds doc ids.
    """

    added_at_t1: frozenset[str]
    added_at_t2: frozenset[str]
    revised_at_t2: frozenset[str]
    deleted_at_t3: frozenset[str]
    taken_down_at_t3: frozenset[str]
    silent_edits: frozenset[str]


def plan(seed: int, documents: list[Document]) -> SnapshotPlan:
    """Draw the timeline: what appears, changes and goes away, and when."""
    rng = Rng(seed, "snapshots")

    # Documents added, revised and deleted between snapshots. Chosen once, from
    # internal documents only, so a regulation never quietly disappears.
    internal = [d.doc_id for d in documents if d.kind == "internal" and "broken" not in d.doc_id]
    web = [d.doc_id for d in documents if d.kind == "web"]

    # Doc 15 section 5's own_card sole support case. This memo is the only
    # statement of its fact, and it is removed at T2, so after that only a prior
    # card still carries the value. Excluded from every other set below, so no
    # other change to the timeline can quietly move it.
    internal = [d for d in internal if d != SOLE_SOURCE_DOC_ID]

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

    return SnapshotPlan(
        added_at_t1=frozenset(added_at_t1),
        added_at_t2=frozenset(added_at_t2),
        revised_at_t2=frozenset(revised_at_t2),
        deleted_at_t3=frozenset(deleted_at_t3),
        taken_down_at_t3=frozenset(taken_down_at_t3),
        silent_edits=frozenset(silent_edits),
    )


def present_at(doc: Document, label: str, timeline: SnapshotPlan) -> bool:
    """Whether a document exists at a label."""
    if doc.doc_id == CAR3_V2 and _before(label, CAR3_V2_PUBLISHED):
        return False
    if doc.doc_id in timeline.added_at_t1 and _before(label, "T1"):
        return False
    if doc.doc_id in timeline.added_at_t2 and _before(label, "T2"):
        return False
    if doc.doc_id in timeline.deleted_at_t3 and not _before(label, "T3"):
        return False
    if doc.doc_id == SOLE_SOURCE_DOC_ID and not _before(label, "T2"):
        return False
    return not (doc.doc_id in timeline.taken_down_at_t3 and not _before(label, "T3"))


def change_at(doc: Document, label: str, timeline: SnapshotPlan) -> str | None:
    """Why this file looks different from the last snapshot, if it does.

    An arrival is reported ahead of an edit, because a file that appears at a
    label has no last snapshot to differ from.
    """
    if doc.doc_id in timeline.added_at_t1 and label == "T1":
        return "added"
    if doc.doc_id in timeline.added_at_t2 and label == "T2":
        return "added"
    if doc.doc_id in timeline.silent_edits and not _before(label, "T2"):
        return "silent_edit"
    if doc.doc_id in timeline.revised_at_t2 and not _before(label, "T2"):
        return "revised"
    return None


def materialise(label: str, documents: list[Document], timeline: SnapshotPlan) -> list[Document]:
    """The documents as they stand at one label, ready to write.

    A revision and a silent edit each append a paragraph, so the body carries
    the change a re-verification has to notice. `Document.body` joins passages
    with a blank line, which is what the manifest hashes, so a materialised
    document hashes to its manifest entry.
    """
    at_label: list[Document] = []
    for doc in documents:
        if not present_at(doc, label, timeline):
            continue
        current = copy.deepcopy(doc)
        if doc.doc_id in timeline.revised_at_t2 and not _before(label, "T2"):
            current.passages.append(_note(doc, "rev-t2", REVISION_NOTE))
        if doc.doc_id in timeline.silent_edits and not _before(label, "T2"):
            current.passages.append(_note(doc, "edit-t2", SILENT_EDIT_NOTE))
        at_label.append(current)
    return at_label


def _note(doc: Document, suffix: str, text: str) -> Passage:
    """The paragraph an edit adds. It plants nothing, because a fact planted
    here would move the answers as well as the hash."""
    return Passage(
        passage_id=f"{doc.doc_id}-{suffix}",
        text=text,
        location={"section": "revision"},
        plants=[],
    )


def build(seed: int, documents: list[Document], facts) -> list[Snapshot]:
    """The four snapshots, each a list of the files that exist at that time."""
    timeline = plan(seed, documents)

    superseded_ids = {f.fact_id for f in facts if f.truth == "superseded"}
    v2_ids = {f.fact_id for f in facts if f.supersedes and f.truth == "true"}
    stable_ids = {f.fact_id for f in facts if f.truth == "true" and f.fact_id not in v2_ids}

    snapshots: list[Snapshot] = []
    for label, at in TIMELINE.items():
        snap = Snapshot(label=label, at=at)

        # Hashed from the materialised documents, so a file that
        # `gen build --snapshot` writes and its entry here state one hash. A
        # revision keeps its locator and moves its hash, which is what a
        # re-verification has to notice.
        for doc in materialise(label, documents, timeline):
            snap.files.append(
                FileState(
                    doc.doc_id,
                    doc.path,
                    content_hash(doc),
                    change_at(doc, label, timeline),
                )
            )

        # Which facts are authoritative here. Before CAR3 v2 applies, the v1
        # values stand; from T3 the v2 values do, and a card citing v1 is stale.
        if _before(label, CAR3_V2_APPLIES):
            snap.facts_in_force = sorted(stable_ids | superseded_ids)
        else:
            snap.facts_in_force = sorted(stable_ids | v2_ids)

        if label == "T2":
            snap.notes.append(
                f"{SOLE_SOURCE_DOC_ID} is removed. The fact it alone carried is now only in a "
                "prior card, which is context and never evidence."
            )
        if label == CAR3_V2_PUBLISHED:
            snap.notes.append(f"{CAR3_V2} is published but does not apply until {CAR3_V2_APPLIES}.")
        if label == CAR3_V2_APPLIES:
            snap.notes.append(
                f"{CAR3_V2} applies. A card written before this that cites {CAR3_V1} for a "
                "changed value is stale."
            )
            snap.notes.append(f"{len(timeline.taken_down_at_t3)} web pages no longer resolve.")

        snapshots.append(snap)

    return snapshots


def _before(label: str, other: str) -> bool:
    order = list(TIMELINE)
    return order.index(label) < order.index(other)


def stale_at_t3(facts) -> list[str]:
    """The facts whose value changed, which is what staleness detection is
    scored against (doc 02 section 10.2)."""
    return sorted(f.fact_id for f in facts if f.truth == "superseded")
