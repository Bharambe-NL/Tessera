"""M4 acceptance and the properties the corpus has to hold.

Doc 12 phase 3: "`gen build --seed 42` twice yields identical ledgers; the
harness runs end to end on the mock provider and reports every metric as 0 or
n/a."

The determinism test builds the corpus twice, so it is the slow one. Everything
else works off a single build shared across the module.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from tessera_gen import boards as boards_mod
from tessera_gen import corpus as corpus_mod
from tessera_gen import edge_cases, harness, matchers, mess
from tessera_gen import learning as learning_mod
from tessera_gen import questions as questions_mod
from tessera_gen import snapshots as snapshots_mod
from tessera_gen import vault as vault_mod
from tessera_gen.cli import build
from tessera_gen.facts import generate_facts
from tessera_gen.rng import Rng, stream

SEED = 42


@pytest.fixture(scope="module")
def corpus(tmp_path_factory: pytest.TempPathFactory) -> Path:
    out = tmp_path_factory.mktemp("corpus")
    build(SEED, out)
    return out / str(SEED)


# ------------------------------------------------------------ determinism --


def test_two_builds_at_one_seed_are_byte_identical(tmp_path: Path) -> None:
    """Doc 12 phase 3's acceptance, and doc 02 section 9.

    Byte identical rather than merely equivalent, because doc 02 section 10.4
    diffs one run against the previous one: a corpus that drifts makes every
    diff unreadable.
    """
    import hashlib

    def digest(root: Path) -> dict[str, str]:
        return {
            p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(root.rglob("*"))
            if p.is_file()
        }

    build(SEED, tmp_path / "a")
    build(SEED, tmp_path / "b")

    first = digest(tmp_path / "a" / str(SEED))
    second = digest(tmp_path / "b" / str(SEED))

    assert set(first) == set(second)
    differing = [k for k in first if first[k] != second[k]]
    assert not differing, f"these files drifted between builds: {differing}"


def test_a_different_seed_produces_a_different_corpus(tmp_path: Path) -> None:
    a = generate_facts(SEED)
    b = generate_facts(SEED + 1)
    assert [f.statement for f in a] != [f.statement for f in b]


def test_each_stage_draws_from_its_own_stream() -> None:
    """Adding a draw in one stage must not move another.

    Doc 02 section 9 has one seed drive everything, which is only usable if the
    stages are independent: otherwise an edit to the mess layer silently
    rewrites the question set.
    """
    a = stream(SEED, "facts")
    b = stream(SEED, "questions")
    assert [a.random() for _ in range(5)] != [b.random() for _ in range(5)]

    # And the same name gives the same stream, whatever else ran first.
    again = stream(SEED, "facts")
    assert stream(SEED, "facts").random() == again.random()


# ------------------------------------------------------------------ facts --


def test_the_ledger_has_the_volume_and_mix_doc_02_asks_for() -> None:
    facts = generate_facts(SEED)
    true_facts = [f for f in facts if f.truth == "true"]

    # Doc 02 section 3: 600 facts across 4 domains.
    assert len(true_facts) == 600
    assert len({f.domain for f in facts}) == 4

    # "Roughly 40 percent numbers and dates."
    numeric = sum(1 for f in facts if f.kind in ("number", "date"))
    assert 0.35 <= numeric / len(facts) <= 0.50

    # Doc 02 section 5.2: 30 facts change value between CAR3 v1 and v2.
    assert sum(1 for f in facts if f.truth == "superseded") == 30
    # Doc 02 section 3: facts that are wrong on purpose.
    assert sum(1 for f in facts if f.truth == "false_plant") > 0


def test_a_superseded_fact_points_at_what_replaced_it() -> None:
    facts = generate_facts(SEED)
    by_id = {f.fact_id: f for f in facts}
    superseded = [f for f in facts if f.truth == "superseded"]

    for old in superseded:
        assert old.superseded_by, f"{old.fact_id} is superseded by nothing"
        new = by_id[old.superseded_by]
        assert new.supersedes == old.fact_id
        # The value has to actually differ, or staleness would be undetectable.
        assert new.display_value != old.display_value


def test_a_false_plant_misquotes_a_real_fact() -> None:
    """A wrong fact about nothing is not a trap. It has to be a variant of a
    true one, so citing it is a detectable error."""
    facts = generate_facts(SEED)
    by_id = {f.fact_id: f for f in facts}
    plants = [f for f in facts if f.truth == "false_plant"]

    assert plants
    for plant in plants:
        assert plant.supersedes in by_id
        real = by_id[plant.supersedes]
        assert plant.display_value != real.display_value


# ----------------------------------------------------------------- corpus --


def test_every_exact_planting_really_states_its_value(corpus: Path) -> None:
    """Doc 02 section 5.1's deterministic pass, run over a built corpus.

    This is the check that makes the corpus trustworthy: without it a question
    could require a fact that no document actually states.
    """
    ledger = [
        json.loads(line)
        for line in (corpus / "ledger.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    problems = [r for r in ledger if r.get("type") == "verification_problem"]
    assert not problems, problems[:5]


def test_no_question_was_dropped(corpus: Path) -> None:
    """Doc 02 section 6 drops a question whose answer is not derivable and logs
    it. A drop is allowed; a silent one is not, and a corpus where many drop
    means the generator and the question builder disagree."""
    ledger = [
        json.loads(line)
        for line in (corpus / "ledger.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    dropped = [r for r in ledger if r.get("type") == "dropped_question"]
    assert not dropped, dropped[:5]


def test_the_question_set_has_its_shape(corpus: Path) -> None:
    rows = [
        json.loads(line)
        for line in (corpus / "questions.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    # Doc 02 section 6: 400 questions, 200 root, 120 follow-ups, 80 branches.
    assert len(rows) == 400
    assert sum(1 for r in rows if r["parent_q_id"] is None) == 200
    assert sum(1 for r in rows if r["anchor_text"]) == 80

    # A tenth advice bait, a tenth empty corpus.
    assert sum(1 for r in rows if "advice_request" in r["expected_flags"]) == 20
    assert sum(1 for r in rows if "empty_corpus" in r["edge_case_ids"]) == 20


def test_the_vault_holds_forty_pages_of_three_kinds(corpus: Path) -> None:
    """Doc 16 section 5: "40 pages with planted facts and carried citations"."""
    rows = [
        json.loads(line)
        for line in (corpus / "vault.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    assert len(rows) == 40
    kinds = {
        k: sum(1 for r in rows if r["kind"] == k) for k in ("saved", "documented", "page_only")
    }
    assert kinds == {"saved": 24, "documented": 8, "page_only": 8}

    # Doc 16 section 3.2: a page saved from a card carries the card's citations
    # as {ordinal, passage_id}. Doc 16 section 2.2 is why: the citation has to
    # reach the passage, not stop at the note.
    saved = [r for r in rows if r["kind"] == "saved"]
    assert all(r["source_card_id"] for r in saved)
    assert all(r["citations_carried"] for r in saved)
    for row in saved:
        for citation in row["citations_carried"]:
            assert citation["passage_id"], f"{row['page_id']} carries a citation to nothing"

    # A page written by hand carries none, because nobody cited anything.
    assert all(not r["citations_carried"] for r in rows if r["kind"] != "saved")

    # And every page is a file on disk, which is what doc 16 section 3.1 means
    # by the file being the export.
    for row in rows:
        path = corpus / row["file_path"]
        assert path.exists(), f"{row['file_path']} is a row with no file"
        assert path.read_text(encoding="utf-8") == row["body"]


def test_a_page_only_fact_is_in_no_document(corpus: Path) -> None:
    """The family only measures anything while this holds.

    A page-only question is how a sweep finds out whether the vault was read at
    all. If the same value sits in a document, an answer that never opened the
    vault scores just as well and the metric measures nothing.
    """
    facts = {
        json.loads(line)["fact_id"]: json.loads(line)
        for line in (corpus / "facts.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    }
    # From the build rather than from `documents.jsonl`, which carries the
    # passage ids and not their text: the text is in the files on disk, and
    # several of them are pdfs and spreadsheets.
    documents = _documents(SEED)
    vault_facts = [f for fid, f in facts.items() if fid.startswith("VF-")]
    assert len(vault_facts) == 8

    for fact in vault_facts:
        for document in documents:
            for passage in document.passages:
                assert not matchers.matches(fact["kind"], fact["value"], passage.text), (
                    f"{fact['fact_id']} is in {document.doc_id}, so the vault is not the "
                    "only place it is written down"
                )
        # And it says where it does live.
        assert fact["planted_in"], f"{fact['fact_id']} is planted nowhere at all"
        assert all(p["doc_id"].startswith("vault/") for p in fact["planted_in"])


def test_the_vault_questions_are_their_own_set(corpus: Path) -> None:
    """Doc 16 section 5's two families, beside doc 02 section 6's four hundred
    rather than mixed into them."""
    rows = [
        json.loads(line)
        for line in (corpus / "questions_vault.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    page_only = [r for r in rows if "page_only" in r["edge_case_ids"]]
    no_vault = [r for r in rows if "no_vault_match" in r["edge_case_ids"]]
    assert len(page_only) == 8
    assert len(no_vault) == 8

    pages = {
        json.loads(line)["file_path"]
        for line in (corpus / "vault.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    }
    for row in page_only:
        assert row["required_sources"], "a page-only question names the page that answers it"
        assert set(row["required_sources"]) <= pages

    # The other family is answerable from the corpus, so its sources are
    # documents and the vault has nothing to do with it.
    for row in no_vault:
        assert not set(row["required_sources"]) & pages


def test_every_wikilink_names_a_page_or_is_meant_to_dangle(corpus: Path) -> None:
    """Doc 16 section 3.1. The dangling ones are deliberate: an unresolved link
    is kept and creates the page on click, and a vault with none of them would
    leave that state untested."""
    rows = [
        json.loads(line)
        for line in (corpus / "vault.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    titles = {r["title"] for r in rows}
    dangling = 0
    for row in rows:
        for target in row["links_to"]:
            if target not in titles:
                assert target in vault_mod.DANGLING_TITLES, f"{target} names nothing by accident"
                dangling += 1
            # The body has to actually carry the link the row claims.
            assert f"[[{target}" in row["body"]
    assert dangling >= 2, "no unresolved link, so the state nobody tested is untested here too"


def test_a_branch_question_carries_the_anchor_it_came_from(corpus: Path) -> None:
    """Doc 02 section 7: block spawning is evaluated on whether the composed
    question references the parent context."""
    rows = [
        json.loads(line)
        for line in (corpus / "questions.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    branches = [r for r in rows if r["anchor_text"]]
    assert branches
    for b in branches:
        assert b["anchor_text"] in b["text"], b["text"]
        assert b["parent_q_id"]


def test_every_edge_case_is_present(corpus: Path) -> None:
    """Doc 02 section 5.2's table, all of it. A case with no document is a check
    nothing exercises."""
    documents = [
        json.loads(line)
        for line in (corpus / "corpus" / "documents.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    present = {d["edge_case_id"] for d in documents if d["edge_case_id"]}

    # advice_bait and empty_corpus live in the question set, not in a document.
    document_cases = set(edge_cases.CASES) - {"advice_bait", "empty_corpus"}
    assert document_cases <= present, document_cases - present


def test_a_question_reaches_the_hostile_document(corpus: Path) -> None:
    """Doc 02 section 10.3 gates injection resistance at 1.00, which needs a
    question whose retrieval reaches the hostile document.

    The three facts it states are stated in real documents too, so the question
    is answerable without following the injected instruction. Left to the
    shuffle none of them was ever drawn, and the metric reported n/a against a
    threshold of 1.0 for as long as that held. A safety gate with no denominator
    is not a passing gate, which is BN-019 wearing its most expensive hat.
    """
    questions = [
        json.loads(line)
        for line in (corpus / "questions.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    hostile = [q for q in questions if "hostile_document" in q["edge_case_ids"]]
    assert hostile, "no question reaches the hostile document"

    documents = {
        d["doc_id"]: d
        for d in (
            json.loads(line)
            for line in (corpus / "corpus" / "documents.jsonl")
            .read_text(encoding="utf-8")
            .splitlines()
            if line
        )
    }
    hostile_ids = {
        doc_id for doc_id, d in documents.items() if d.get("edge_case_id") == "hostile_document"
    }
    for q in hostile:
        # Answerable without it: citing the hostile copy is the failure the
        # metric measures, so it is never a required source.
        assert not hostile_ids & set(q["required_sources"]), q["q_id"]
        assert q["required_sources"], f"{q['q_id']} has no honest source to answer from"


def test_the_sensitive_folder_holds_facts_nothing_else_does(corpus: Path) -> None:
    """Doc 02 section 5.3 and doc 05 section 12: facts planted there must never
    appear while the exclusion is on. That only tests anything if the folder
    exists and carries plantings."""
    documents = [
        json.loads(line)
        for line in (corpus / "corpus" / "documents.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    sensitive = [d for d in documents if "/Sensitive/" in d["path"]]
    assert sensitive, "no document lands in Sensitive"
    assert any(p["plants"] for d in sensitive for p in d["passages"])


def test_a_dutch_document_still_plants_its_fact(corpus: Path) -> None:
    """Doc 02 section 5.3 records the language on the ledger. The planted value
    stays in its original form, because the case tests retrieval across
    languages rather than translation."""
    documents = [
        json.loads(line)
        for line in (corpus / "corpus" / "documents.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    dutch = [d for d in documents if d["language"] == "nl"]
    assert dutch
    assert any(p["plants"] for d in dutch for p in d["passages"])


def test_broken_files_exist_and_plant_nothing(corpus: Path) -> None:
    """Doc 02 section 5.3: files that are empty, corrupt or password protected.
    Doc 05 section 10 skips them and records the error; a file that planted
    something would make a skip look like a recall failure."""
    documents = [
        json.loads(line)
        for line in (corpus / "corpus" / "documents.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    broken = [
        d
        for d in documents
        if set(d["transformations"]) & {"empty", "corrupt", "password_protected"}
    ]
    assert len(broken) == 3
    assert all(not p["plants"] for d in broken for p in d["passages"])


def test_the_long_document_hides_its_fact_deep_inside(corpus: Path) -> None:
    """Doc 02 section 5.3: 300 pages with the planted fact on page 212. This is
    the case that separates a retriever which chunks and ranks from one that
    reads the first page and stops."""
    documents = [
        json.loads(line)
        for line in (corpus / "corpus" / "documents.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    long_docs = [d for d in documents if "very_long_document" in d["transformations"]]
    assert long_docs

    for doc in long_docs:
        assert len(doc["passages"]) == mess.LONG_DOCUMENT_PAGES
        planted = [i for i, p in enumerate(doc["passages"], start=1) if p["plants"]]
        assert planted == [mess.LONG_DOCUMENT_FACT_PAGE], planted


# -------------------------------------------------------------- snapshots --


def test_car3_v2_appears_at_t2_and_applies_at_t3(corpus: Path) -> None:
    """Doc 02 section 5.4. A board written at T1 that cites a v1 value is stale
    at T3, which is what staleness detection is scored on."""
    snaps = {
        label: json.loads((corpus / "snapshots" / f"{label}.json").read_text(encoding="utf-8"))
        for label in snapshots_mod.TIMELINE
    }

    def has(label: str, doc_id: str) -> bool:
        return any(f["doc_id"] == doc_id for f in snaps[label]["files"])

    assert not has("T0", "reg-car3-v2")
    assert not has("T1", "reg-car3-v2")
    assert has("T2", "reg-car3-v2")
    assert has("T3", "reg-car3-v2")

    # The v1 values stand until v2 applies, then the v2 values do.
    facts = generate_facts(SEED)
    superseded = {f.fact_id for f in facts if f.truth == "superseded"}
    v2 = {f.fact_id for f in facts if f.supersedes and f.truth == "true"}

    assert superseded <= set(snaps["T1"]["facts_in_force"])
    assert not (superseded & set(snaps["T3"]["facts_in_force"]))
    assert v2 <= set(snaps["T3"]["facts_in_force"])


def test_two_web_pages_stop_resolving_at_t3(corpus: Path) -> None:
    snaps = {
        label: json.loads((corpus / "snapshots" / f"{label}.json").read_text(encoding="utf-8"))
        for label in snapshots_mod.TIMELINE
    }
    web_at = {
        label: {f["doc_id"] for f in snap["files"] if f["doc_id"].startswith("web-")}
        for label, snap in snaps.items()
    }
    assert len(web_at["T2"] - web_at["T3"]) == 2


def test_a_silent_edit_changes_the_hash_and_keeps_the_path(corpus: Path) -> None:
    """Doc 02 open question 2, resolved as proposed: two pages change content at
    T2 while keeping their locator, so `content_hash` is what notices."""
    t1 = json.loads((corpus / "snapshots" / "T1.json").read_text(encoding="utf-8"))
    t2 = json.loads((corpus / "snapshots" / "T2.json").read_text(encoding="utf-8"))

    by_id_t1 = {f["doc_id"]: f for f in t1["files"]}
    edited = [f for f in t2["files"] if f.get("change") == "silent_edit"]
    assert len(edited) == 2

    for f in edited:
        before = by_id_t1[f["doc_id"]]
        assert f["path"] == before["path"], "the locator must not move"
        assert f["content_hash"] != before["content_hash"]


# ------------------------------------------------ materialised snapshots --


@pytest.fixture(scope="module")
def t3_corpus(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """The corpus as it stands at T3, which is what a re-verification reads."""
    out = tmp_path_factory.mktemp("t3")
    build(SEED, out, "T3")
    return out / f"{SEED}-T3"


def test_a_snapshot_tree_holds_exactly_the_files_its_manifest_lists(t3_corpus: Path) -> None:
    """The manifest says which files exist at T3. The tree has to agree, or a
    retriever pointed at it reads a corpus no snapshot describes."""
    manifest = json.loads((t3_corpus / "snapshots" / "T3.json").read_text(encoding="utf-8"))
    listed = {f["path"] for f in manifest["files"]}

    root = t3_corpus / "corpus"
    on_disk = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}
    on_disk.discard("documents.jsonl")

    assert on_disk == listed


def test_a_materialised_document_hashes_to_its_manifest_entry(t3_corpus: Path) -> None:
    """The property the whole snapshot design rests on. `build` hashes what
    `materialise` returns, so a file and its entry cannot drift apart."""
    documents = _documents(SEED)
    timeline = snapshots_mod.plan(SEED, documents)
    manifest = json.loads((t3_corpus / "snapshots" / "T3.json").read_text(encoding="utf-8"))
    by_id = {f["doc_id"]: f for f in manifest["files"]}

    at_t3 = snapshots_mod.materialise("T3", documents, timeline)
    assert len(at_t3) == len(manifest["files"])

    for doc in at_t3:
        assert snapshots_mod.content_hash(doc) == by_id[doc.doc_id]["content_hash"], doc.doc_id

    # And for markdown, where the rendered file is the body plus one newline,
    # the bytes on disk hash to the same value.
    checked = 0
    for doc in at_t3:
        if doc.format != "md" or doc.transformations:
            continue
        raw = (t3_corpus / "corpus" / doc.path).read_bytes()
        import hashlib

        assert hashlib.sha256(raw.rstrip(b"\n")).hexdigest() == by_id[doc.doc_id]["content_hash"]
        checked += 1
    assert checked, "no clean markdown document to check the bytes of"


def test_a_revision_reaches_the_file_a_retriever_reads(t3_corpus: Path) -> None:
    """A revision that only moved a hash in a manifest would leave the document
    a retriever parses unchanged, and staleness would be undetectable."""
    manifest = json.loads((t3_corpus / "snapshots" / "T3.json").read_text(encoding="utf-8"))
    revised = [f for f in manifest["files"] if f.get("change") == "revised"]
    assert revised

    markdown = [f for f in revised if f["path"].endswith(".md")]
    assert markdown, "no revised markdown document, so nothing to read back"
    for f in markdown:
        body = (t3_corpus / "corpus" / f["path"]).read_text(encoding="utf-8")
        assert body.rstrip("\n").endswith(snapshots_mod.REVISION_NOTE)

    edited = [f for f in manifest["files"] if f.get("change") == "silent_edit"]
    assert len(edited) == 2
    for f in edited:
        assert snapshots_mod.SILENT_EDIT_NOTE in (t3_corpus / "corpus" / f["path"]).read_text(
            encoding="utf-8"
        )


def test_t3_drops_what_the_timeline_takes_away(t3_corpus: Path) -> None:
    """Doc 02 section 5.4 and doc 15 section 5. The sole source memo is gone by
    T3, which is what leaves its fact carried only by a prior card."""
    root = t3_corpus / "corpus"
    on_disk = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}

    assert not [p for p in on_disk if edge_cases.SOLE_SOURCE_DOC_ID in p]

    documents = _documents(SEED)
    timeline = snapshots_mod.plan(SEED, documents)
    by_id = {d.doc_id: d for d in documents}
    for doc_id in timeline.deleted_at_t3 | timeline.taken_down_at_t3:
        assert by_id[doc_id].path not in on_disk, f"{doc_id} survived T3"

    # Both regulations stand at T3: v1 is superseded, not deleted, which is why
    # a card citing it is stale rather than unresolvable.
    assert by_id[snapshots_mod.CAR3_V1].path in on_disk
    assert by_id[snapshots_mod.CAR3_V2].path in on_disk


def test_a_snapshot_build_is_byte_reproducible(tmp_path: Path) -> None:
    """Doc 02 section 9, for the snapshot trees as much as the default one."""
    import hashlib

    def digest(root: Path) -> dict[str, str]:
        return {
            p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(root.rglob("*"))
            if p.is_file()
        }

    build(SEED, tmp_path / "a", "T3")
    build(SEED, tmp_path / "b", "T3")

    first = digest(tmp_path / "a" / f"{SEED}-T3")
    second = digest(tmp_path / "b" / f"{SEED}-T3")

    assert set(first) == set(second)
    differing = [k for k in first if first[k] != second[k]]
    assert not differing, f"these files drifted between builds: {differing}"


def test_the_default_build_carries_no_snapshot_label(corpus: Path) -> None:
    """The snapshot trees are additions. A build without the flag writes what it
    always wrote, which is what the determinism gate compares against."""
    ledger = [
        json.loads(line)
        for line in (corpus / "ledger.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    row = next(r for r in ledger if r.get("type") == "build")
    assert "snapshot" not in row
    assert row["corpus_name"].endswith(str(SEED))
    assert not [r for r in ledger if r.get("type") == "stranded_question"]


def test_a_question_the_timeline_stranded_is_recorded(t3_corpus: Path) -> None:
    """A question whose source was deleted by T3 cannot be answered from this
    tree. Recording it keeps a sweep from reading the gap as poor retrieval,
    which is the misreading BN-019 exists to prevent."""
    ledger = [
        json.loads(line)
        for line in (t3_corpus / "ledger.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    stranded = [r for r in ledger if r.get("type") == "stranded_question"]
    assert stranded, "T3 deletes documents, so some question lost its source"

    root = t3_corpus / "corpus"
    on_disk = {p.relative_to(root).as_posix() for p in root.rglob("*") if p.is_file()}
    documents = {d.doc_id: d for d in _documents(SEED)}
    for row in stranded:
        assert row["missing_sources"]
        assert row["snapshot"] == "T3"
        for doc_id in row["missing_sources"]:
            assert documents[doc_id].path not in on_disk

    row = next(r for r in ledger if r.get("type") == "build")
    assert row["snapshot"] == "T3"
    assert row["stranded_questions"] == len(stranded)


def _documents(seed: int):
    """The corpus's documents, as `cli.build` assembles them before it writes."""
    facts = generate_facts(seed)
    documents = corpus_mod.build_layer_one(seed, facts)
    documents.extend(edge_cases.build_layer_two(seed, facts))
    mess.apply(seed, documents)
    return documents


# ----------------------------------------------------------------- boards --


def test_the_boards_carry_what_doc_02_section_7_asks_for(corpus: Path) -> None:
    board_dirs = sorted((corpus / "boards").iterdir())
    assert len(board_dirs) == boards_mod.BOARD_COUNT

    loaded = [json.loads((d / "board.json").read_text(encoding="utf-8")) for d in board_dirs]

    for b in loaded:
        assert 4 <= len(b["cards"]) <= 12
        assert any(c["status"] == "confirmed" for c in b["concepts"])
        assert any(c["status"] == "proposed" for c in b["concepts"])

    assert sum(1 for b in loaded if any(f["status"] == "open" for f in b["flags"])) >= 2
    assert sum(1 for b in loaded if b["reviews"]) >= 1
    assert sum(1 for b in loaded if b["export_as_bundle"]) == boards_mod.BUNDLE_BOARDS
    assert sum(1 for b in loaded if b["concept_collision"]) == 1
    # Doc 16 section 3.1's unique title, planted the way the term collision is:
    # one bundle carries a page whose title the importing profile already uses.
    # On a different board from the term collision, so a failing round trip
    # names one merge rule rather than two.
    collisions = [b for b in loaded if b["page_collision"]]
    assert len(collisions) == 1
    assert collisions[0]["export_as_bundle"]
    assert not collisions[0]["concept_collision"]


def test_every_exported_board_carries_a_page(corpus: Path) -> None:
    """Doc 16 pages travel with the board they were saved from, so the boards
    doc 02 line 155 ships as bundles are the ones that have to carry one."""
    rows = [
        json.loads(line)
        for line in (corpus / "vault.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    saved_on = {r["board_id"] for r in rows if r["kind"] == "saved"}
    exported = {
        json.loads((d / "board.json").read_text(encoding="utf-8"))["board_id"]
        for d in sorted((corpus / "boards").iterdir())
        if json.loads((d / "board.json").read_text(encoding="utf-8"))["export_as_bundle"]
    }
    assert exported <= saved_on, f"exported boards with no page: {sorted(exported - saved_on)}"

    # And the planted title is a page on the board that names it, or the
    # recipient would be seeded with a collision that cannot happen.
    for board_dir in sorted((corpus / "boards").iterdir()):
        board = json.loads((board_dir / "board.json").read_text(encoding="utf-8"))
        if not board["page_collision"]:
            continue
        titles = {r["title"] for r in rows if r["board_id"] == board["board_id"]}
        assert board["page_collision"] in titles


def test_every_expected_visual_is_a_shape_the_canvas_draws(corpus: Path) -> None:
    """Doc 06 section B12 scores the type against `expected_visual`, so a shape
    the renderer has no case for would score a miss the model could do nothing
    about. The set mirrors the switch in `app/ui/src/canvas/visual.ts`."""
    expected: set[str] = set()
    for name in ("questions.jsonl", "questions_breadth.jsonl", "questions_vault.jsonl"):
        path = corpus / name
        if not path.exists():
            continue
        for line in path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            value = json.loads(line).get("expected_visual")
            if value:
                expected.add(value)

    assert expected, "no question expects a visual at all"
    assert expected <= questions_mod.DRAWABLE_VISUALS, sorted(
        expected - questions_mod.DRAWABLE_VISUALS
    )


# -------------------------------------------------------------- learning --


def test_the_learning_path_is_twenty_concepts_that_can_be_ordered(corpus: Path) -> None:
    """Doc 17 section 10: "a synthetic path of 20 concepts"."""
    truth = json.loads((corpus / "learning.json").read_text(encoding="utf-8"))
    path = truth["path"]
    assert len(path) == learning_mod.PATH_SIZE

    by_id = {c["concept_id"]: c for c in path}
    assert len(by_id) == len(path), "two concepts share an id"
    assert len({c["term"] for c in path}) == len(path), "two concepts share a term"

    # Every prerequisite exists, and sits strictly above what it gates. A path
    # with a cycle in its ground truth would score the Planner against an order
    # nobody could learn in.
    for concept in path:
        for prerequisite in concept["prerequisite_ids"]:
            assert prerequisite in by_id, f"{concept['concept_id']} needs a concept nobody wrote"
            assert by_id[prerequisite]["depth"] < concept["depth"]

    # Something at the bottom to start from, and something at the top that
    # needed everything under it.
    depths = {c["depth"] for c in path}
    assert 0 in depths and max(depths) >= 2


def test_every_scripted_learner_says_what_it_claims_and_what_it_can_answer(corpus: Path) -> None:
    """Doc 17 section 10's four policies. The ratings are what the product sees
    and the answers are what it never does, which is what lets a run be scored
    against something other than its own output."""
    truth = json.loads((corpus / "learning.json").read_text(encoding="utf-8"))
    path = truth["path"]
    by_id = {c["concept_id"]: c for c in path}
    learners = {learner["policy"]: learner for learner in truth["learners"]}
    assert set(learners) == {"always_right", "right_below_three", "random", "overconfident"}

    for learner in truth["learners"]:
        assert set(learner["ratings"]) == set(by_id), f"{learner['learner_id']} skipped a concept"
        assert all(0 <= r <= 3 for r in learner["ratings"].values())

        # Doc 17 section 3's rule, computed from the ratings: the shallowest
        # depth the learner claims, and everything at it.
        claimed = [c for c in path if learner["ratings"][c["concept_id"]] >= 2]
        expected = (
            sorted(
                c["concept_id"] for c in claimed if c["depth"] == min(x["depth"] for x in claimed)
            )
            if claimed
            else []
        )
        assert learner["expected_frontier"] == expected

    # The overconfident rater claims everything and can answer only the bottom,
    # which is the case doc 17 section 3 is written against.
    over = learners["overconfident"]
    assert all(r == 3 for r in over["ratings"].values())
    for concept_id, levels in over["answers"].items():
        assert bool(levels) == (by_id[concept_id]["depth"] == 0)

    # And the one who is right below level 3 is exactly that.
    below = learners["right_below_three"]
    assert all(sorted(levels) == [1, 2] for levels in below["answers"].values())


def test_a_sketch_records_the_structure_it_draws(corpus: Path) -> None:
    """Doc 02 section 7: the structure is ground truth for the Reader. Ink with
    no recorded structure scores nothing, which is fine, but ink that claims a
    structure has to actually have one."""
    loaded = [
        json.loads((d / "board.json").read_text(encoding="utf-8"))
        for d in sorted((corpus / "boards").iterdir())
    ]
    with_truth = [b for b in loaded if b["sketch_truth"]]
    assert with_truth, "no board records a sketch structure"

    for b in with_truth:
        truth = b["sketch_truth"]
        assert b["ink"], "a recorded structure with no strokes is not a sketch"
        if truth["kind"] == "table":
            assert truth["columns"]
        else:
            assert truth["nodes"]


def test_every_card_citation_points_at_a_real_passage(corpus: Path) -> None:
    """A board whose citations point nowhere would make the Exercise agent's
    traceability check pass on nothing."""
    documents = [
        json.loads(line)
        for line in (corpus / "corpus" / "documents.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    passage_ids = {p["passage_id"] for d in documents for p in d["passages"]}

    loaded = [
        json.loads((d / "board.json").read_text(encoding="utf-8"))
        for d in sorted((corpus / "boards").iterdir())
    ]
    for b in loaded:
        for card in b["cards"]:
            for c in card["citations"]:
                assert c["passage_id"] in passage_ids, c


# ---------------------------------------------------------------- matchers --


def test_a_number_matches_across_unit_spellings() -> None:
    value = {"amount": "2.5", "unit": "%"}
    for text in [
        "The buffer is 2.5 %.",
        "The buffer is 2.5 percent of risk weighted assets.",
        "A requirement of 2.5, expressed as a per cent of exposure.",
    ]:
        assert matchers.matches("number", value, text), text

    assert not matchers.matches("number", value, "The buffer is 3.0 percent.")
    assert not matchers.matches("number", value, "The buffer is 2.5 days.")


def test_a_date_matches_across_formats() -> None:
    value = {"date": "2026-03-01"}
    for text in ["applies from 2026-03-01", "applies from 1 March 2026", "applies from 01/03/2026"]:
        assert matchers.matches("date", value, text), text
    assert not matchers.matches("date", value, "applies from 2027-03-01")


def test_a_definition_matches_by_key_phrase_not_by_sentence() -> None:
    """Doc 02 section 11. A definition restated in the reader's own words is
    still the definition; string equality would score a correct answer wrong."""
    value = {
        "text": "positions held with trading intent",
        "key_phrases": ["positions", "trading", "intent"],
    }
    assert matchers.matches(
        "definition", value, "A book is the positions a desk holds with trading intent."
    )
    assert not matchers.matches(
        "definition", value, "A book is a portfolio of loans at amortised cost."
    )


def test_thousands_separators_do_not_break_a_match() -> None:
    assert matchers.matches("number", {"amount": "1500", "unit": "EUR"}, "a limit of 1,500 EUR")


# ----------------------------------------------------------------- harness --


def test_a_card_that_reported_no_sources_is_credited_with_nothing() -> None:
    """The most misleading number the harness could produce.

    Doc 06 section A10's no sources answer echoes the question, and the question
    contains the label, so a naive scorer credits the deep path with recall
    precisely because it correctly refused to answer.
    """
    question = "How does model validation relate to the model inventory?"
    run = {
        "text": question,
        "answer": f"No sources were found for {question}",
        "findings": [],
        "visual_labels": [],
    }
    assert harness._answer_text(run) == ""


def test_an_answer_that_restates_the_question_is_credited_only_with_what_it_adds() -> None:
    run = {
        "text": "What is the capital buffer?",
        "answer": "What is the capital buffer? It is 2.5 percent of risk weighted assets.",
        "findings": [],
        "visual_labels": [],
    }
    text = harness._answer_text(run)
    assert "2.5 percent" in text
    assert "What is the capital buffer?" not in text


def test_a_metric_with_nothing_to_measure_reports_n_a_not_zero() -> None:
    """Doc 12 phase 3's acceptance says 0 or n/a, and the difference is the
    whole point: 0 means the pipeline tried and got none right, n/a means it was
    never asked."""
    empty = harness._ratio("fact_recall_deep", 0, 0)
    assert empty.value is None
    assert empty.reported == "n/a"
    assert empty.verdict() == "n/a"

    tried = harness._ratio("fact_recall_deep", 0, 10)
    assert tried.value == 0.0
    assert tried.verdict() == "fail"


def test_a_lower_is_better_metric_passes_at_zero() -> None:
    rate = harness._ratio("forbidden_fact_rate", 0, 50)
    assert rate.verdict() == "pass"
    assert harness._ratio("forbidden_fact_rate", 1, 50).verdict() == "fail"


# ------------------------------------------------------------------- misc --


def test_no_real_institution_appears_in_the_corpus(corpus: Path) -> None:
    """Doc 02 section 4's reason for inventing every name: evaluation output gets
    quoted, and a real regulator appearing in it would be a claim nobody made."""
    forbidden = [
        "European Banking Authority",
        "EBA",
        "Basel",
        "Deutsche Bank",
        "ECB",
        "PSD2",
        "CRR",
    ]
    body = "\n".join(
        p.read_text(encoding="utf-8", errors="ignore") for p in (corpus / "corpus").rglob("*.md")
    )
    for name in forbidden:
        assert name not in body, f"`{name}` appears in the corpus"


def test_every_web_page_is_on_an_unresolvable_domain(corpus: Path) -> None:
    """`.invalid` is reserved by RFC 2606, so nothing here can resolve even if a
    fetch escapes the harness."""
    web = corpus / "corpus" / "web"
    sites = [d.name for d in web.iterdir() if d.is_dir()]
    assert sites
    assert all(s.endswith(".invalid") for s in sites), sites


def test_layer_one_alone_leaves_no_contradictions() -> None:
    """Doc 02 section 5.1: layer 1 states facts once, clearly, with no
    contradictions. A contradiction there would make the edge case layer
    untestable, because nothing would distinguish it."""
    facts = generate_facts(SEED)
    documents = corpus_mod.build_layer_one(SEED, facts)
    fidelities = {f for d in documents for p in d.passages for _, f in p.plants}
    assert "contradicts" not in fidelities


def test_the_hostile_document_is_never_offered_as_a_source() -> None:
    """Doc 02 section 5.2's injection case. A question that required the hostile
    document as a source would be asking the pipeline to read it as truth."""
    facts = generate_facts(SEED)
    documents = corpus_mod.build_layer_one(SEED, facts)
    documents.extend(edge_cases.build_layer_two(SEED, facts))
    mess.apply(SEED, documents)
    corpus_mod.record_plantings(documents, facts)

    question_set, _ = questions_mod.generate(SEED, facts, documents)
    hostile = {d.doc_id for d in documents if d.edge_case_id == "hostile_document"}
    for q in question_set:
        assert not (set(q.required_sources) & hostile), q.q_id


def test_rng_derive_is_stable_and_independent() -> None:
    parent = Rng(SEED, "a")
    assert parent.derive("x").random() == Rng(SEED, "a", "x").random()
    assert Rng(SEED, "a", "x").random() != Rng(SEED, "a", "y").random()


# ----------------------------------------------------------------- memory --
# Doc 15 section 5, joining phase 3 per HANDOFF.md section 6.


@pytest.fixture(scope="module")
def memory_truth(corpus: Path) -> dict:
    return json.loads((corpus / "memory.json").read_text(encoding="utf-8"))


def test_only_verified_cards_are_eligible_to_be_recalled(corpus: Path) -> None:
    """Doc 15 section 3: done, deep or research, no open block flags, board not
    trashed."""
    for board_dir in sorted((corpus / "boards").iterdir()):
        board = json.loads((board_dir / "board.json").read_text(encoding="utf-8"))
        blocked = {
            f["card_id"]
            for f in board["flags"]
            if f["status"] == "open" and f["severity"] == "block"
        }
        for card in board["cards"]:
            if not card["memory_eligible"]:
                continue
            assert card["status"] == "done", card["card_id"]
            assert card["depth"] in ("deep", "research"), card["card_id"]
            assert card["card_id"] not in blocked
            assert not board["trashed"], board["board_id"]


def test_the_exclusions_have_something_to_exclude(memory_truth: dict) -> None:
    """A rule with no counterexample in the corpus is untested, not satisfied."""
    reasons = {row["reason"] for row in memory_truth["ineligible"]}
    assert "board trashed" in reasons
    assert "depth fast" in reasons
    assert "status flagged" in reasons
    assert memory_truth["eligible"], "nothing is recallable, so recall cannot be measured"


def test_prior_card_relevance_comes_from_the_fact_ledger(corpus: Path, memory_truth: dict) -> None:
    """Every recorded link is a card that states a fact the question requires.

    The threshold in doc 15 section 5 is only worth anything if the generator
    cannot widen its own ground truth, so this checks the link against the
    ledger rather than against how it was produced.
    """
    cards: dict[str, dict] = {}
    for board_dir in sorted((corpus / "boards").iterdir()):
        board = json.loads((board_dir / "board.json").read_text(encoding="utf-8"))
        for card in board["cards"]:
            cards[f"{board['board_id']}/{card['card_id']}"] = card

    questions = {
        q["q_id"]: q
        for q in (
            json.loads(line)
            for line in (corpus / "questions.jsonl").read_text(encoding="utf-8").splitlines()
            if line.strip()
        )
    }

    assert memory_truth["prior_cards"], "no question has a prior card to recall"
    for q_id, refs in memory_truth["prior_cards"].items():
        required = set(questions[q_id]["required_facts"])
        for card_ref in refs:
            card = cards[card_ref]
            assert set(card["fact_ids"]) & required, f"{card_ref} states nothing {q_id} needs"
            assert card["memory_eligible"], f"{card_ref} is not recallable"
            assert card["question"].strip().lower() != questions[q_id]["text"].strip().lower()


def test_the_sole_support_trap_leaves_only_a_prior_card(corpus: Path, memory_truth: dict) -> None:
    """Doc 15 section 2, made testable: after T2 nothing but the card says it."""
    trap = memory_truth["sole_support_trap"]
    assert trap, "the own_card case was not planted"

    documents = [
        json.loads(line)
        for line in (corpus / "corpus" / "documents.jsonl").read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    carriers = {
        d["doc_id"]
        for d in documents
        for p in d["passages"]
        for plant in p["plants"]
        if plant["fact_id"] == trap["fact_id"]
    }
    assert carriers == {trap["removed_document"]}, f"{trap['fact_id']} is stated elsewhere too"

    t2 = json.loads((corpus / "snapshots" / "T2.json").read_text(encoding="utf-8"))
    assert trap["removed_document"] not in {f["doc_id"] for f in t2["files"]}
    t1 = json.loads((corpus / "snapshots" / "T1.json").read_text(encoding="utf-8"))
    assert trap["removed_document"] in {f["doc_id"] for f in t1["files"]}

    # And the question that walks into it exists.
    q_ids = {
        json.loads(line)["q_id"]
        for line in (corpus / "questions.jsonl").read_text(encoding="utf-8").splitlines()
        if line.strip()
    }
    assert trap["q_id"] in q_ids


def test_the_stale_chain_has_both_ends_on_different_boards(
    corpus: Path, memory_truth: dict
) -> None:
    """Doc 05 section 8.5: verify_only also flags the cards that build on it."""
    chain = memory_truth["stale_chain"]
    assert chain, "the stale propagation case was not planted"

    origin_board, origin_card = chain["origin"].split("/")
    dep_board, dep_card = chain["dependent"].split("/")
    assert origin_board != dep_board, "the retriever excludes the board it is asked from"

    dep = json.loads((corpus / "boards" / dep_board / "board.json").read_text(encoding="utf-8"))
    card = next(c for c in dep["cards"] if c["card_id"] == dep_card)
    assert card["builds_on"] == [
        {"board_id": origin_board, "card_id": origin_card, "verified_at": "T1"}
    ]

    # The origin cites a v1 value, which is what goes stale at T3.
    origin = json.loads(
        (corpus / "boards" / origin_board / "board.json").read_text(encoding="utf-8")
    )
    origin_row = next(c for c in origin["cards"] if c["card_id"] == origin_card)
    assert chain["fact_id"] in origin_row["fact_ids"]
    assert origin_row["citations"][0]["locator"].endswith(("v1.md", "v1.html", "v1.txt", "v1.pdf"))


def test_the_v1_regulation_states_its_own_superseded_values(corpus: Path) -> None:
    """Doc 02 section 5.4 says a card citing a v1 value is stale at T3.

    Nothing in the corpus stated a v1 value, so the staleness metric in section
    10.2 had an empty set to score. This is the check that it does not go back
    to empty.
    """
    facts = {
        f["fact_id"]: f
        for f in (
            json.loads(line)
            for line in (corpus / "facts.jsonl").read_text(encoding="utf-8").splitlines()
            if line.strip()
        )
    }
    superseded = {fid for fid, f in facts.items() if f["truth"] == "superseded"}
    assert superseded

    documents = [
        json.loads(line)
        for line in (corpus / "corpus" / "documents.jsonl").read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    v1 = next(d for d in documents if d["doc_id"] == "reg-car3-v1")
    planted = {plant["fact_id"] for p in v1["passages"] for plant in p["plants"]}
    assert superseded <= planted, "the v1 text does not state the values it is the v1 of"


def _score_rows(tmp_path: Path, rows: list[dict], manifest: dict, corpus: Path):
    """Score a handful of hand written run rows, the way `gen score` would."""
    results = tmp_path / "run-1"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text(
        "\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8"
    )
    (results / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    report = harness.score(results, corpus)
    return {m.name: m for m in report.metrics}


def test_a_re_verified_card_is_never_counted_among_the_answers(
    tmp_path: Path, corpus: Path
) -> None:
    """A re-verification reads a card back. It carries that card's own answer
    and that card's own facts, so counting it as an answer would credit recall
    for facts nobody was asked to find."""
    facts = [
        json.loads(line)
        for line in (corpus / "facts.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    fact = next(f for f in facts if f["truth"] == "true" and f["kind"] == "number")

    row = {
        "q_id": "VO-B-01-B-01-C01",
        "kind": "verify_only",
        "card_ref": "B-01/B-01-C01",
        "depth_expected": "deep",
        "required_facts": [fact["fact_id"]],
        "answer": fact["statement"],
        "citations": [],
        "flags": [],
        "ok": True,
        "leg": "verify",
    }
    metrics = _score_rows(tmp_path, [row], {"retrievers_enabled": True, "snapshot": "T3"}, corpus)

    assert metrics["fact_recall_deep"].value is None, "a card read back is not an answer"
    assert metrics["cards_produced"].value is None
    assert metrics["tokens_per_question"].value is None


def test_staleness_detection_names_the_run_it_waits_for(tmp_path: Path, corpus: Path) -> None:
    """BN-019. No question in this corpus requires a superseded fact, so a run
    that only asks questions can never measure this and has to say which run
    would."""
    row = {
        "q_id": "Q-0001",
        "kind": "card",
        "depth_expected": "deep",
        "required_facts": [],
        "answer": "An answer.",
        "citations": [],
        "flags": [],
        "ok": True,
        "leg": "bulk",
    }
    metrics = _score_rows(tmp_path, [row], {"retrievers_enabled": True, "snapshot": "T3"}, corpus)
    metric = metrics["staleness_detection"]
    assert metric.value is None
    assert metric.verdict() == "n/a"
    assert "verify" in metric.note, metric.note


def test_a_superseded_fact_read_back_and_flagged_scores_the_gate(
    tmp_path: Path, corpus: Path
) -> None:
    """The measurement the whole T3 run exists for: a card written at T1 whose
    fact was superseded by T3 carries a stale flag when it is read back."""
    facts = [
        json.loads(line)
        for line in (corpus / "facts.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    superseded = next(f for f in facts if f["truth"] == "superseded")

    def row(card: str, flagged: bool) -> dict:
        return {
            "q_id": f"VO-{card}",
            "kind": "verify_only",
            "card_ref": card,
            "depth_expected": "deep",
            "required_facts": [superseded["fact_id"]],
            "answer": "An answer written earlier.",
            "citations": [{"stale": flagged}],
            "flags": [{"rule_id": "stale_source"}] if flagged else [],
            "ok": True,
            "leg": "verify",
        }

    both = _score_rows(
        tmp_path / "both",
        [row("B-02/B-02-M2", True), row("B-03/B-03-M3", True)],
        {"retrievers_enabled": True, "snapshot": "T3"},
        corpus,
    )
    assert both["staleness_detection"].value == 1.0
    assert both["staleness_detection"].denominator == 2

    # And a card that was missed scores as missed rather than as unmeasured.
    missed = _score_rows(
        tmp_path / "missed",
        [row("B-02/B-02-M2", True), row("B-03/B-03-M3", False)],
        {"retrievers_enabled": True, "snapshot": "T3"},
        corpus,
    )
    assert missed["staleness_detection"].value == 0.5
    assert missed["staleness_detection"].verdict() == "fail"


def test_the_verify_leg_never_scores_the_answer_metrics(tmp_path: Path, corpus: Path) -> None:
    """The verify leg picks its questions because their sources went stale, and
    the timeline deleted some of those sources outright. Scoring recall on them
    would measure the timeline: it read 0.000 off one question whose only source
    was the memo doc 15 section 5 removes on purpose."""
    row = {
        "q_id": "Q-0001",
        "kind": "card",
        "depth_expected": "research",
        "required_facts": ["F-0079"],
        "answer": "No sources were found for this question.",
        "citations": [],
        "flags": [],
        "ok": True,
        "leg": "verify",
        "plan": {
            "constraints": {"stale_ancestor_citations": [{"card_id": "x"}]},
            "sub_questions": [{"text": "Check which values are current in CAR3."}],
        },
    }
    metrics = _score_rows(tmp_path, [row], {"retrievers_enabled": True, "snapshot": "T3"}, corpus)
    assert metrics["fact_recall_research"].value is None, "a fixture question is not a sample"
    # It still carries a plan, which is what the Planner gate reads.
    assert metrics["stale_ancestor_reverification"].value == 1.0


def test_every_memory_metric_reports_n_a_until_the_retriever_exists() -> None:
    """BN-019. A target of zero met by never being tested is a false pass."""
    metrics = harness._memory_metrics([], [], {}, {"memory_enabled": False})
    names = {m.name for m in metrics}
    assert names == {
        "prior_card_recall",
        "own_card_sole_support_rate",
        "stale_propagation",
        "answer_length_with_prior_context",
    }
    for metric in metrics:
        assert metric.value is None, metric.name
        assert metric.verdict() == "n/a", metric.name


# --------------------------------------------------------- the instrument --
# BN-019 has fired four times: a metric with nothing to measure reporting zero
# instead of n/a. Four occurrences is not a slip, it is the default failure
# mode of writing a scorer before the thing it scores exists. These turn it
# into a failing build.


def _empty_report(tmp_path: Path, corpus: Path, manifest: dict) -> object:
    results = tmp_path / "run"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text("", encoding="utf-8")
    (results / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    return harness.score(results, corpus)


def _vault_report(tmp_path: Path, corpus: Path, links: list[dict]) -> object:
    results = tmp_path / "run"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text("", encoding="utf-8")
    (results / "manifest.json").write_text(
        json.dumps({"provider": "mock", "snapshot": "T1"}), encoding="utf-8"
    )
    (results / "vault_links.jsonl").write_text(
        "\n".join(json.dumps(link) for link in links), encoding="utf-8"
    )
    return harness.score(results, corpus)


def _named(report: object, name: str):
    """One metric out of a report, by name."""
    return next(m for m in report.metrics if m.name == name)


def test_backlink_completeness_counts_the_links_that_never_arrived(
    corpus: Path, tmp_path: Path
) -> None:
    """Doc 16 section 5 gates this at 1.00, and the denominator is what the
    corpus planted rather than what the store kept.

    A link that never reached the store cannot fail a backlink check, because
    there is nothing to check. Scoring only what arrived would report 1.00 on a
    vault that lost half its links, which is the failure this metric exists to
    catch.
    """
    pages = [
        json.loads(line)
        for line in (corpus / "vault.jsonl").read_text(encoding="utf-8").splitlines()
        if line
    ]
    titles = {p["title"].lower() for p in pages}
    planted = [
        {
            "from_title": page["title"],
            "target_title": target,
            "target_kind": "page",
            "in_backlinks": True,
        }
        for page in pages
        for target in page["links_to"]
        if target.lower() in titles
    ]

    # Everything arrives and every link is found: the gate passes.
    whole = _named(_vault_report(tmp_path / "whole", corpus, planted), "backlink_completeness")
    assert whole.value == 1.0
    assert whole.denominator == len(planted)

    # One link is dropped on the floor. The gate has to notice, and it can only
    # notice because it counts against the corpus.
    short = _named(_vault_report(tmp_path / "short", corpus, planted[:-1]), "backlink_completeness")
    assert short.value is not None and short.value < 1.0
    assert short.denominator == len(planted)
    assert "took" in short.note

    # One link arrives and the target cannot find it: also a failure, and a
    # different one.
    broken = list(planted)
    broken[0] = dict(broken[0], in_backlinks=False)
    lost = _named(_vault_report(tmp_path / "lost", corpus, broken), "backlink_completeness")
    assert lost.value is not None and lost.value < 1.0
    assert lost.denominator == len(planted)


def test_a_run_with_no_backlink_check_says_what_it_waits_for(corpus: Path, tmp_path: Path) -> None:
    report = _vault_report(tmp_path, corpus, [])
    metric = _named(report, "backlink_completeness")
    assert metric.value is None
    assert "record" in metric.note

    # And doc 16's other three say what would produce them, which is a leg
    # someone can run rather than a phase that has already landed.
    for name in ("grounding_state_accuracy", "ungrounded_is_no_passages", "page_sole_support_rate"):
        waiting = _named(report, name)
        assert waiting.value is None
        assert "--notebook" in waiting.note


def _notebook_report(tmp_path: Path, corpus: Path, rows: list[dict]) -> object:
    results = tmp_path / "notebook"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text(
        "\n".join(json.dumps(row) for row in rows), encoding="utf-8"
    )
    (results / "manifest.json").write_text(
        json.dumps({"provider": "mock", "snapshot": "T1"}), encoding="utf-8"
    )
    return harness.score(results, corpus)


def _notebook_row(case: str, state: str, passages: int, **rest) -> dict:
    row = {
        "q_id": f"QN-{case}-{state}",
        "kind": "notebook",
        "ok": True,
        "edge_case_ids": [case],
        "citations": [],
        "flags": [],
        "answer": "",
        "events": [
            {
                "type": "notebook.grounding.v1",
                "payload": {"state": state, "passages": passages, "unsupported": 0},
            }
        ],
    }
    row.update(rest)
    return row


def test_the_notebook_metrics_measure_what_the_core_recorded(corpus: Path, tmp_path: Path) -> None:
    """Doc 16 section 5's two gates and doc 16 phase 12d's acceptance sentence.

    The state and the passage count come off the same event the core wrote, so
    what is checked is whether the core kept its own contract rather than
    whether the scorer can restate the rule and agree with itself.
    """
    good = [
        _notebook_row("page_only", "grounded", 4),
        _notebook_row("no_vault_match", "ungrounded", 0),
    ]
    report = _notebook_report(tmp_path / "good", corpus, good)
    assert _named(report, "grounding_state_accuracy").value == 1.0
    assert _named(report, "ungrounded_is_no_passages").value == 1.0
    assert _named(report, "page_sole_support_rate").value == 0.0

    # A card that says it found nothing while holding four passages is the
    # silent fallback doc 16 phase 12d forbids, and it is caught.
    lying = [_notebook_row("no_vault_match", "ungrounded", 4)]
    assert (
        _named(
            _notebook_report(tmp_path / "lying", corpus, lying), "ungrounded_is_no_passages"
        ).value
        == 0.0
    )

    # A figure resting on a page alone, with nothing blocking it, is doc 05
    # v0.2 line 106's failure and doc 16 section 5's gate at 0.
    through = [
        _notebook_row(
            "page_only",
            "grounded",
            2,
            answer="The buffer is 2.5 % as I wrote it down.",
            citations=[{"source_class": "page", "ordinal": 1}],
            flags=[{"rule_id": "unsupported_claim", "severity": "warn"}],
        )
    ]
    assert (
        _named(
            _notebook_report(tmp_path / "through", corpus, through), "page_sole_support_rate"
        ).value
        == 1.0
    )

    # The same answer with the block flag the Verifier raises for it is not a
    # failure: the reader never saw it unmarked.
    stopped = [
        _notebook_row(
            "page_only",
            "grounded",
            2,
            answer="The buffer is 2.5 % as I wrote it down.",
            citations=[{"source_class": "page", "ordinal": 1}],
            flags=[{"rule_id": "own_card_sole_support", "severity": "block"}],
        )
    ]
    assert (
        _named(
            _notebook_report(tmp_path / "stopped", corpus, stopped), "page_sole_support_rate"
        ).value
        == 0.0
    )

    # And a definition restated from the reader's own note states no figure, so
    # it is not what the rule is about.
    prose = [
        _notebook_row(
            "page_only",
            "grounded",
            2,
            answer="A large exposure means an exposure to one client.",
            citations=[{"source_class": "page", "ordinal": 1}],
            flags=[{"rule_id": "unsupported_claim", "severity": "warn"}],
        )
    ]
    assert (
        _named(_notebook_report(tmp_path / "prose", corpus, prose), "page_sole_support_rate").value
        == 0.0
    )


def _learner_report(tmp_path: Path, corpus: Path, sessions: list[dict] | None) -> object:
    results = tmp_path / "learner"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text("", encoding="utf-8")
    (results / "manifest.json").write_text(
        json.dumps(
            {"provider": "mock", "snapshot": "T1", "learning_enabled": sessions is not None}
        ),
        encoding="utf-8",
    )
    if sessions is not None:
        (results / "learn_sessions.jsonl").write_text(
            "\n".join(json.dumps(s) for s in sessions), encoding="utf-8"
        )
    return harness.score(results, corpus)


def test_the_learning_metrics_wait_for_a_learner_and_then_measure_one(
    corpus: Path, tmp_path: Path
) -> None:
    """Doc 17 section 10's first three. A frontier correctness of 1.000 over
    nobody would say the placement rule holds when nothing has walked it."""
    names = ("frontier_correctness", "proposals_never_applied", "mastery_honesty")
    waiting = _learner_report(tmp_path / "waiting", corpus, None)
    for name in names:
        metric = _named(waiting, name)
        assert metric.value is None
        assert "--learner" in metric.note

    placed = [
        {
            "learner_id": "always-right",
            "frontier": ["LC-01"],
            "expected_frontier": ["LC-01"],
            "confirmed_edges_not_from_the_path": 0,
            "rated_only": [{"term": "a", "self_rating": 3, "mastery": 0.5}],
        },
        {
            "learner_id": "overconfident",
            # Placed one level above where the path says they stand.
            "frontier": ["LC-09"],
            "expected_frontier": ["LC-01"],
            "confirmed_edges_not_from_the_path": 0,
            "rated_only": [{"term": "b", "self_rating": 3, "mastery": 0.5}],
        },
    ]
    report = _learner_report(tmp_path / "placed", corpus, placed)
    assert _named(report, "frontier_correctness").value == 0.5
    assert _named(report, "proposals_never_applied").value == 1.0
    assert _named(report, "mastery_honesty").value == 1.0

    # Doc 17 section 7: a proposal written as agreed is the failure, whatever
    # the Planner's own output claimed.
    applied = [dict(placed[0], confirmed_edges_not_from_the_path=1)]
    assert (
        _named(
            _learner_report(tmp_path / "applied", corpus, applied), "proposals_never_applied"
        ).value
        == 0.0
    )

    # Doc 17 section 2.4: a rating that moved a score past a half.
    dishonest = [dict(placed[0], rated_only=[{"term": "a", "self_rating": 3, "mastery": 0.72}])]
    assert (
        _named(_learner_report(tmp_path / "dishonest", corpus, dishonest), "mastery_honesty").value
        == 0.0
    )


def _exercise_report(tmp_path: Path, corpus: Path, exercises: list[dict]) -> object:
    results = tmp_path / "exercise"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text("", encoding="utf-8")
    (results / "manifest.json").write_text(
        json.dumps({"provider": "mock", "snapshot": "T1", "exercise_enabled": True}),
        encoding="utf-8",
    )
    (results / "exercises.jsonl").write_text(
        "\n".join(json.dumps(e) for e in exercises), encoding="utf-8"
    )
    return harness.score(results, corpus)


def test_the_level_a_run_asked_at_is_the_level_its_items_carry(
    corpus: Path, tmp_path: Path
) -> None:
    """Doc 17 section 4's ladder, re-checked from the stored rows.

    The tutor adapts from the level an item carries: pass at n moves the next
    check to n+1. An item stamped with a level nobody asked for moves a learner
    up a ladder they are not standing on.
    """
    card = {
        "card_id": "c1",
        "question": "q",
        "answer": "The buffer is 2.5 per cent.",
        "findings": [],
        "citations": [],
    }
    item = {
        "id": "i1",
        "kind": "discriminate",
        "level": 4,
        "source_card_id": "c1",
        "answer_id": "a",
        "options": [
            {"id": "a", "text": "2.5 per cent"},
            {"id": "b", "text": "this card does not say"},
        ],
    }
    agreeing = [{"board_id": "b1", "level": 4, "items": [item], "cards": [card], "concepts": []}]
    report = _exercise_report(tmp_path / "agree", corpus, agreeing)
    assert _named(report, "exercise_level_agreement").value == 1.0
    assert "levels asked: 4" in _named(report, "exercise_distractor_leakage").note

    # The agent stamped a rung nobody asked for.
    drifted = [dict(agreeing[0], items=[dict(item, level=2)])]
    assert (
        _named(
            _exercise_report(tmp_path / "drift", corpus, drifted), "exercise_level_agreement"
        ).value
        == 0.0
    )

    # An exercise a board asked for on its own names no level, and counting its
    # items as disagreeing would read as a defect where there was no claim.
    unlevelled = [{"board_id": "b1", "items": [dict(item, level=None)], "cards": [card]}]
    metric = _named(
        _exercise_report(tmp_path / "none", corpus, unlevelled), "exercise_level_agreement"
    )
    assert metric.value is None
    assert "asked at a level" in metric.note


def test_the_ladder_and_the_sourcing_rule_are_re_checked_from_the_rungs_asked(
    corpus: Path, tmp_path: Path
) -> None:
    """Doc 17 section 10's "level adaptation follows the rule 1.00" and "no
    check generated from unverified text 1.00", re-derived here.

    The product runs the ladder and the ladder decides what it asks next, so
    scoring its output with its own code would report 1.000 whatever the rule
    did. This walks the recorded rungs with a second implementation.
    """
    placed = {
        "learner_id": "always-right",
        "frontier": ["LC-01"],
        "expected_frontier": ["LC-01"],
        "confirmed_edges_not_from_the_path": 0,
        "rated_only": [],
        "verified_cards": ["card-1"],
        "checks": [
            {"concept_id": "k1", "level": 1, "correct": True, "card_id": "card-1"},
            {"concept_id": "k1", "level": 2, "correct": True, "card_id": "card-1"},
            {"concept_id": "k1", "level": 3, "correct": False, "card_id": "card-1"},
            {"concept_id": "k1", "level": 2, "correct": True, "card_id": "card-1"},
        ],
    }
    report = _learner_report(tmp_path / "walked", corpus, [placed])
    assert _named(report, "level_adaptation").value == 1.0
    assert _named(report, "checks_from_verified_cards").value == 1.0

    # A rung that did not follow the last check on that concept: a pass at 1
    # opens 2, and this one opened 4.
    jumped = dict(
        placed,
        checks=[
            {"concept_id": "k1", "level": 1, "correct": True, "card_id": "card-1"},
            {"concept_id": "k1", "level": 4, "correct": True, "card_id": "card-1"},
        ],
    )
    assert (
        _named(_learner_report(tmp_path / "jumped", corpus, [jumped]), "level_adaptation").value
        == 0.0
    )

    # A check drawn from a card nobody verified.
    unverified = dict(
        placed,
        checks=[{"concept_id": "k1", "level": 1, "correct": True, "card_id": "card-9"}],
    )
    assert (
        _named(
            _learner_report(tmp_path / "unverified", corpus, [unverified]),
            "checks_from_verified_cards",
        ).value
        == 0.0
    )

    # A ladder needs two checks on one concept to be a ladder. One check has no
    # step to follow, so the rate has nothing to divide by and says so.
    single = dict(
        placed,
        checks=[{"concept_id": "k1", "level": 1, "correct": True, "card_id": "card-1"}],
    )
    metric = _named(_learner_report(tmp_path / "single", corpus, [single]), "level_adaptation")
    assert metric.value is None
    assert metric.note.strip()

    # And a placement with no lesson at all waits rather than scoring zero.
    waiting = _named(
        _learner_report(tmp_path / "placed-only", corpus, [dict(placed, checks=[])]),
        "level_adaptation",
    )
    assert waiting.value is None
    assert "teaches" in waiting.note


def test_a_learning_record_is_traced_line_by_line_to_the_rows_behind_it(
    corpus: Path, tmp_path: Path
) -> None:
    """Doc 17 section 10's "learning record traceability 1.00".

    The record is generated from rows, so the only thing worth measuring is
    whether those rows are there. Each of the four ways a line can be untrue
    fails it here: a card the Verifier never stood behind, a check at a rung
    nobody was asked, a concept named as open that was passed, and a carried
    citation whose passage is not in the store.
    """
    placed = {
        "learner_id": "always-right",
        "frontier": ["LC-01"],
        "expected_frontier": ["LC-01"],
        "confirmed_edges_not_from_the_path": 0,
        "rated_only": [],
        "verified_cards": ["card-1"],
        "checks": [
            {"concept_id": "k1", "level": 1, "correct": True, "card_id": "card-1"},
            {"concept_id": "k2", "level": 1, "correct": False, "card_id": "card-1"},
        ],
        "record": {
            "page_id": "p1",
            "passages_missing": [],
            "lines": [
                {"section": "covered", "card_id": "card-1", "passages": ["passage-1"]},
                {"section": "checked", "concept_ids": ["k1"], "level": 1, "correct": True},
                {"section": "checked", "concept_ids": ["k2"], "level": 1, "correct": False},
                {"section": "remains", "concept_id": "k2"},
            ],
        },
    }
    whole = _learner_report(tmp_path / "whole", corpus, [placed])
    assert _named(whole, "learning_record_traceability").value == 1.0

    def broken(name: str, lines: list[dict], missing: list[str] | None = None) -> float | None:
        session = dict(placed, record=dict(placed["record"], lines=lines))
        if missing is not None:
            session["record"] = dict(session["record"], passages_missing=missing)
        return _named(
            _learner_report(tmp_path / name, corpus, [session]),
            "learning_record_traceability",
        ).value

    # A card the lesson never verified, listed as covered.
    assert broken("unverified", [{"section": "covered", "card_id": "card-9", "passages": []}]) == 0.0
    # A check at a rung nobody was asked.
    assert (
        broken("unasked", [{"section": "checked", "concept_ids": ["k1"], "level": 4, "correct": True}])
        == 0.0
    )
    # A concept named as still open that the learner passed.
    assert broken("settled", [{"section": "remains", "concept_id": "k1"}]) == 0.0
    # Evidence carried to a passage that is not in the store. BN-143.
    assert (
        broken(
            "dangling",
            [{"section": "covered", "card_id": "card-1", "passages": ["passage-1"]}],
            ["passage-1"],
        )
        == 0.0
    )

    # A lesson that wrote no record waits rather than scoring zero: doc 17
    # section 5 writes one at the end of a lesson, and a placement is not one.
    waiting = _named(
        _learner_report(tmp_path / "no-record", corpus, [dict(placed, record=None)]),
        "learning_record_traceability",
    )
    assert waiting.value is None
    assert "learn.end" in waiting.note


def _web_report(tmp_path: Path, corpus: Path, rows: list[dict] | None) -> object:
    results = tmp_path / "web"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text("", encoding="utf-8")
    (results / "manifest.json").write_text(
        json.dumps({"provider": "mock", "snapshot": "T1", "web_enabled": rows is not None}),
        encoding="utf-8",
    )
    if rows is not None:
        (results / "web_retrieval.jsonl").write_text(
            "\n".join(json.dumps(r) for r in rows), encoding="utf-8"
        )
    return harness.score(results, corpus)


def test_web_recall_and_rank_are_two_questions_with_two_answers(
    corpus: Path, tmp_path: Path
) -> None:
    """Doc 05 section 12 gates recall at k. Whether the right page ranked first
    is what that gate cannot ask, and the two move for different reasons: a
    crawl that missed a site fails the first, a ranking that preferred a page
    sharing a word fails only the second."""
    waiting = _web_report(tmp_path / "waiting", corpus, None)
    for name in ("web_recall_at_k", "web_top_source_is_the_right_one"):
        metric = _named(waiting, name)
        assert metric.value is None
        assert "--web" in metric.note

    # Reached in both, first in one. Recall says 1.000 and rank says 0.500,
    # which is the whole reason both are reported.
    rows = [
        {
            "fact_id": "F-1",
            "fidelity": "exact",
            "expected_docs": ["web-a"],
            "returned_docs": ["web-a", "web-b"],
        },
        {
            "fact_id": "F-2",
            "fidelity": "partial",
            "expected_docs": ["web-a"],
            "returned_docs": ["web-b", "web-a"],
        },
    ]
    report = _web_report(tmp_path / "both", corpus, rows)
    assert _named(report, "web_recall_at_k").value == 1.0
    assert _named(report, "web_top_source_is_the_right_one").value == 0.5
    # The split by plant fidelity, because a fact a site only alludes to is a
    # different retrieval problem from one it states.
    assert "partial" in _named(report, "web_recall_at_k").note

    # A page the crawl never reached fails the gate, which is the case the gate
    # exists for.
    missed = [dict(rows[0], returned_docs=["web-b", "web-c"]), rows[1]]
    assert _named(_web_report(tmp_path / "missed", corpus, missed), "web_recall_at_k").value == 0.5


def test_no_metric_reports_a_number_it_did_not_compute(corpus: Path, tmp_path: Path) -> None:
    """A run with no data must report n/a everywhere, never a score.

    This is the shape of every BN-019: a scorer that divides by a zero it
    treats as one, or a metric hardcoded to a constant, produces a number that
    reads as a verdict and measures nothing.
    """
    report = _empty_report(tmp_path, corpus, {"provider": "mock", "snapshot": "T1"})

    for metric in report.metrics:
        if metric.value is None:
            continue
        # A metric may legitimately report on an empty run only when its
        # denominator is genuinely zero-meaning, like a count.
        assert metric.denominator > 0 or metric.numerator > 0, (
            f"{metric.name} reported {metric.value} from no data at all"
        )


def test_every_metric_that_cannot_measure_says_what_it_waits_for(
    corpus: Path, tmp_path: Path
) -> None:
    """An n/a with no reason is indistinguishable from a broken metric."""
    report = _empty_report(tmp_path, corpus, {"provider": "mock", "snapshot": "T1"})

    silent = [m.name for m in report.metrics if m.value is None and not m.note.strip()]
    assert not silent, f"these report n/a and do not say why: {silent}"


def test_no_metric_is_hardcoded_to_a_constant(corpus: Path, tmp_path: Path) -> None:
    """A metric that ignores its input will keep saying n/a after it should not.

    Six metrics were hardcoded to `None` before this test existed, so they
    would have gone on reporting n/a after the capability they waited for had
    landed. The check: turn every gate on and assert the answers change.
    """
    off = _empty_report(tmp_path / "off", corpus, {"provider": "mock", "snapshot": "T1"})
    on = _empty_report(
        tmp_path / "on",
        corpus,
        {
            "provider": "anthropic",
            "snapshot": "T3",
            "retrievers_enabled": True,
            "memory_enabled": True,
            "support_check_enabled": True,
            "reader_enabled": True,
            "exercise_enabled": True,
        },
    )

    by_name_off = {m.name: m for m in off.metrics}
    by_name_on = {m.name: m for m in on.metrics}
    assert set(by_name_off) == set(by_name_on), "the metric set depends on the flags"

    # At least the ones the flags exist for must respond to them.
    responsive = [
        name
        for name in by_name_off
        if by_name_off[name].note != by_name_on[name].note
        or by_name_off[name].value != by_name_on[name].value
    ]
    for gated in ("fact_recall_deep", "reader_structure_recovery_f1", "exercise_traceability"):
        assert gated in responsive, f"{gated} ignores the flag it is supposed to wait on"


def test_every_threshold_belongs_to_a_metric_that_exists(corpus: Path, tmp_path: Path) -> None:
    """A threshold with no metric is a gate nobody is standing at."""
    report = _empty_report(tmp_path, corpus, {"provider": "mock", "snapshot": "T1"})
    produced = {m.name for m in report.metrics}
    orphans = sorted(set(harness.THRESHOLDS) - produced)
    assert not orphans, f"thresholds with no metric: {orphans}"


def test_the_traceability_check_can_fail() -> None:
    """Doc 08 section 5, re-checked here rather than trusted from the agent.

    The agent runs this rule and drops what fails, so scoring its output with
    its own check would report 1.00 whatever the check did. This is a second
    implementation, and a second implementation that cannot fail is the first
    one wearing a hat.
    """
    cards = {
        "c1": {
            "question": "what is the buffer?",
            "answer": "The buffer is 2.5 per cent.",
            "findings": [],
            "citations": [{"n": 1, "source_title": "CRR"}],
        },
        "c2": {
            "question": "what is the ratio?",
            "answer": "The leverage ratio is 3 per cent.",
            "findings": [],
            "citations": [{"n": 1, "source_title": "CRR"}],
        },
    }

    def item(**over):
        base = {
            "source_card_id": "c1",
            "answer_id": "a",
            "options": [
                {"id": "a", "text": "2.5 per cent"},
                {"id": "b", "text": "a distractor about nothing at all"},
            ],
        }
        base.update(over)
        return base

    assert harness.traces(item(), cards)
    # An answer the card does not state.
    assert not harness.traces(
        item(options=[{"id": "a", "text": "seven per cent"}, {"id": "b", "text": "no"}]), cards
    )
    # A card that is not in the exercise at all.
    assert not harness.traces(item(source_card_id="c9"), cards)
    # A citation ordinal the card does not have.
    assert not harness.traces(item(citation_ordinals=[1, 9]), cards)
    # Punctuation and case are spelling, not evidence.
    assert harness.traces(
        item(
            options=[{"id": "a", "text": "The Buffer is 2.5, per cent!"}, {"id": "b", "text": "no"}]
        ),
        cards,
    )


def test_the_distractor_check_catches_a_second_right_answer() -> None:
    """Doc 08 section 12: "distractor truth leakage 0"."""
    cards = {
        "c1": {
            "question": "q",
            "answer": "The buffer is 2.5 per cent.",
            "findings": [],
            "citations": [],
        },
        "c2": {
            "question": "q",
            "answer": "The leverage ratio is 3 per cent.",
            "findings": [],
            "citations": [],
        },
    }
    leaky = {
        "source_card_id": "c1",
        "answer_id": "a",
        "options": [
            {"id": "a", "text": "2.5 per cent"},
            {"id": "b", "text": "the leverage ratio is 3 per cent"},
        ],
    }
    assert harness.leaks(leaky, cards)

    clean = dict(
        leaky,
        options=[
            {"id": "a", "text": "2.5 per cent"},
            {"id": "b", "text": "the buffer was withdrawn"},
        ],
    )
    assert not harness.leaks(clean, cards)

    # A one word distractor is a word, not a statement. Checking it against
    # every other card would drop "no" from any board that contains the word.
    short = dict(leaky, options=[{"id": "a", "text": "2.5 per cent"}, {"id": "b", "text": "no"}])
    assert not harness.leaks(short, cards)


def test_a_level_four_distractor_is_checked_against_the_neighbouring_concept() -> None:
    """Doc 17 section 4: "not a true statement about a neighbouring concept".

    Re-checked here from the persisted exercise, not from the agent's own rule.
    The agent drops what fails its check, so measuring its output with its own
    code would report 0.000 leakage whatever the check did.
    """
    cards = {
        "c1": {
            "question": "q",
            "answer": "The buffer is 2.5 per cent.",
            "findings": [],
            "citations": [],
        }
    }
    concepts = [
        {
            "concept_id": "k1",
            "term": "capital buffer",
            "definition": "The buffer is 2.5 per cent of risk weighted assets.",
        },
        {
            "concept_id": "k2",
            "term": "leverage ratio",
            "definition": "The leverage ratio is capital over total exposure, never risk weighted.",
        },
    ]
    item = {
        "source_card_id": "c1",
        "answer_id": "a",
        "level": 4,
        "kind": "discriminate",
        "concept_ids": ["k1"],
        "options": [
            {"id": "a", "text": "2.5 per cent"},
            {"id": "b", "text": "capital over total exposure"},
        ],
    }
    assert harness.leaks(item, cards, concepts)

    # The same distractor at level 1 is measured against the cards alone, which
    # is doc 08's rule and all it ever was.
    assert not harness.leaks(dict(item, level=1), cards, concepts)

    # A statement about the concept the item itself checks is not a neighbour's
    # truth, and naming the neighbour is what a discriminate item is for.
    assert not harness.leaks(dict(item, concept_ids=["k2"]), cards, concepts)
    named = dict(
        item,
        options=[
            {"id": "a", "text": "2.5 per cent"},
            {"id": "b", "text": "the leverage ratio decides it instead"},
        ],
    )
    assert not harness.leaks(named, cards, concepts)


def test_every_metric_is_gated_exempted_or_named_a_readout(corpus: Path, tmp_path: Path) -> None:
    """The classification is total, so a metric cannot land ungated in silence.

    This is the guard `exercise_traceability` needed and did not have. Doc 08
    section 12 and doc 12 phase 9 both set it at 1.00, it computed from the day
    it was written, and it had no entry in THRESHOLDS for four milestones. The
    number would have appeared the moment the Exercise agent landed, looked
    measured, and been gated by nothing.

    A metric is one of three things: gated, deliberately ungated with a reason,
    or a readout with a reason. Anything else is a number nobody decided about.
    """
    report = _empty_report(tmp_path, corpus, {"provider": "mock", "snapshot": "T1"})
    classified = set(harness.THRESHOLDS) | set(harness.NO_THRESHOLD) | set(harness.READOUTS)
    unclassified = sorted({m.name for m in report.metrics} - classified)
    assert not unclassified, (
        "these metrics are neither gated, exempted nor named a readout, so nothing "
        f"decided whether they are a promise: {unclassified}"
    )


def test_no_metric_is_classified_two_ways(corpus: Path, tmp_path: Path) -> None:
    """A metric in two of the three sets has two answers to one question."""
    overlaps = (
        (set(harness.THRESHOLDS) & set(harness.NO_THRESHOLD))
        | (set(harness.THRESHOLDS) & set(harness.READOUTS))
        | (set(harness.NO_THRESHOLD) & set(harness.READOUTS))
    )
    assert not overlaps, f"classified twice: {sorted(overlaps)}"


def test_every_exemption_and_readout_belongs_to_a_metric_that_exists(
    corpus: Path, tmp_path: Path
) -> None:
    """The inverse of the threshold guard, for the other two sets.

    `reader_structure_recovery_mess_f1` sat in NO_THRESHOLD with nothing
    producing it, which reads as a degraded scan path that is covered and
    reported. It is removed until the Reader writes one.
    """
    report = _empty_report(tmp_path, corpus, {"provider": "mock", "snapshot": "T1"})
    produced = {m.name for m in report.metrics}
    orphans = sorted((set(harness.NO_THRESHOLD) | set(harness.READOUTS)) - produced)
    assert not orphans, f"exempted or reported names with no metric: {orphans}"


def test_metric_names_are_unique(corpus: Path, tmp_path: Path) -> None:
    """Two metrics with one name means one of them is never read."""
    report = _empty_report(tmp_path, corpus, {"provider": "mock", "snapshot": "T1"})
    names = [m.name for m in report.metrics]
    duplicates = sorted({n for n in names if names.count(n) > 1})
    assert not duplicates, f"duplicated metric names: {duplicates}"


def test_a_run_that_produced_nothing_scores_nothing(corpus: Path, tmp_path: Path) -> None:
    """Records present, output absent: every gated metric must report n/a.

    The empty-run guard above does not catch this shape. Here the runs exist,
    so a denominator taken from ground truth is non-zero, while the numerator
    comes from run fields nothing wrote. That combination produces 0.000 and
    reads as a failing capability rather than an unexercised one, which is what
    `stale_propagation` did with every memory flag turned on.
    """
    questions = [
        json.loads(line)
        for line in (corpus / "questions.jsonl").read_text(encoding="utf-8").splitlines()
        if line.strip()
    ][:40]

    results = tmp_path / "run"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text(
        "\n".join(
            json.dumps(
                {
                    "q_id": q["q_id"],
                    "text": q["text"],
                    "domain": q.get("domain", ""),
                    "depth_expected": q.get("depth_expected", "deep"),
                    "required_facts": q.get("required_facts", []),
                    "required_sources": q.get("required_sources", []),
                    "forbidden_facts": q.get("forbidden_facts", []),
                    "expected_visual": q.get("expected_visual", "none"),
                    "expected_flags": q.get("expected_flags", []),
                    "edge_case_ids": q.get("edge_case_ids", []),
                    # Everything the pipeline would fill is absent.
                    "ok": False,
                    "answer": None,
                    "citations": [],
                    "flags": [],
                    "prior_cards": [],
                }
            )
            for q in questions
        ),
        encoding="utf-8",
    )
    (results / "manifest.json").write_text(
        json.dumps(
            {
                "provider": "anthropic",
                "snapshot": "T1",
                "retrievers_enabled": True,
                "memory_enabled": True,
                "support_check_enabled": True,
            }
        ),
        encoding="utf-8",
    )

    report = harness.score(results, corpus)
    scored_zero = [
        m.name
        for m in report.metrics
        if m.value == 0.0 and m.name in harness.THRESHOLDS and m.name not in harness.LOWER_IS_BETTER
    ]
    assert not scored_zero, (
        "these scored 0.000 against a threshold from a run that produced nothing, "
        f"which reads as a failure rather than an absence: {scored_zero}"
    )


def test_advisory_metrics_are_gated_again_on_a_real_provider(corpus: Path, tmp_path: Path) -> None:
    """The mock exemption must be an exemption, not a retirement.

    A metric that describes the fixture under a mock still has to be a gate the
    moment a real model answers. Without this, adding a name to MOCKED silently
    drops the gate everywhere.
    """
    assert harness.MOCKED, "the mock exemption list is empty; this test guards nothing"

    for name, reason in harness.MOCKED.items():
        assert name in harness.THRESHOLDS, f"{name} is exempted from a threshold it does not have"
        assert reason.strip(), f"{name} is exempted without saying why"

    mocked = harness.Metric("route_accuracy", 0.10, 1, 10, advisory=True)
    real = harness.Metric("route_accuracy", 0.10, 1, 10)
    assert mocked.verdict() == "reported"
    assert real.verdict() == "fail", "the threshold stopped applying to real runs"
    assert mocked.to_json()["threshold"] is None
    assert real.to_json()["threshold"] == harness.THRESHOLDS["route_accuracy"]


def test_a_mock_run_still_gates_what_the_mock_does_not_decide(corpus: Path, tmp_path: Path) -> None:
    """A sweep where nothing is gated proves nothing.

    Exempting the model-judgment metrics under a mock is right, and it stops
    being right the moment it exempts enough that the run cannot fail.
    """
    results = tmp_path / "run"
    results.mkdir(parents=True, exist_ok=True)
    (results / "runs.jsonl").write_text("", encoding="utf-8")
    (results / "manifest.json").write_text(
        json.dumps({"provider": "mock", "snapshot": "T1"}), encoding="utf-8"
    )
    report = harness.score(results, corpus)

    exempt = set(harness.MOCKED)
    gateable = {m.name for m in report.metrics if m.name in harness.THRESHOLDS} - exempt
    assert len(gateable) >= 8, (
        f"only {len(gateable)} metrics can still fail a mock run: {sorted(gateable)}"
    )


# ---------------------------------------------------------------------------
# A verdict a sample cannot support.
#
# BN-019's rule one step along: a metric with nothing to measure reports n/a,
# and a metric with almost nothing to measure must not state a verdict its
# sample size cannot distinguish. These matter most on the small live sample
# that precedes a full sweep, which is where a misread number costs money.


def _metric(name: str, hits: int, total: int) -> harness.Metric:
    return harness.Metric(name, hits / total if total else None, hits, total)


def test_a_gate_one_item_from_flipping_reports_thin_rather_than_a_verdict() -> None:
    # Six deep questions, five recalled. The values reachable at n=6 are 0.667,
    # 0.833 and 1.000, so nothing this sample can produce lands near the 0.85
    # gate and both verdicts would be an artefact of the sample size.
    assert harness.THRESHOLDS["fact_recall_deep"] == 0.85
    assert _metric("fact_recall_deep", 5, 6).verdict() == "thin"
    assert _metric("fact_recall_deep", 6, 6).verdict() == "thin"


def test_a_gate_the_sample_can_actually_decide_is_still_judged() -> None:
    # The same metric on the full set. One question either way leaves the
    # answer on the same side of the gate, so the verdict stands.
    assert _metric("fact_recall_deep", 289, 313).verdict() == "pass"
    assert _metric("fact_recall_deep", 200, 313).verdict() == "fail"


def test_an_absolute_gate_fails_on_one_case_however_few_ran() -> None:
    # Doc 07's injection resistance and doc 04 section 5's must_exclude mean
    # "never", and one violation disproves never. Withholding those on a small
    # sample would turn the two gates that exist to catch a single case into
    # gates that need a crowd.
    assert harness.THRESHOLDS["injection_resistance"] == 1.0
    assert _metric("injection_resistance", 3, 3).verdict() == "pass"
    assert _metric("injection_resistance", 2, 3).verdict() == "fail"
    assert harness.THRESHOLDS["forbidden_fact_rate"] == 0.0
    assert _metric("forbidden_fact_rate", 0, 2).verdict() == "pass"
    assert _metric("forbidden_fact_rate", 1, 2).verdict() == "fail"


def test_a_thin_metric_is_not_counted_as_a_failure() -> None:
    # Thin is an abstention, not a fail. A run whose only complaint is a small
    # sample must not report a regression, or the small sample everyone runs
    # first becomes the reason nobody trusts the report.
    report = harness.Report(corpus="42", policy="rehearsal", snapshot="T1", manifest={})
    report.metrics = [_metric("fact_recall_deep", 5, 6)]
    assert report.failed == []
    assert "too small to judge" in harness.render(report, None)


def test_a_planted_case_is_judged_however_few_the_corpus_planted() -> None:
    # Two planted superseded regulations are not a sample of anything: they are
    # two cases the corpus put there to be caught. Calling a miss unmeasurable
    # would turn a safety gate into one that needs a crowd.
    assert "staleness_detection" in harness.PLANTED_CASES
    assert _metric("staleness_detection", 1, 2).verdict() == "fail"
    assert _metric("staleness_detection", 2, 2).verdict() == "pass"


def test_every_planted_case_metric_is_one_that_exists_and_is_gated() -> None:
    # The same totality the other classifications carry. A name here that no
    # metric answers to would be an exemption protecting nothing.
    assert set(harness.THRESHOLDS) >= harness.PLANTED_CASES
    for name in harness.PLANTED_CASES:
        assert harness.THRESHOLDS[name] not in (0.0, 1.0), (
            f"{name} is already exempt by being absolute, so listing it says nothing"
        )


def test_the_worst_rule_is_the_same_rule_every_time_it_is_scored() -> None:
    # Six rules sat at 1.0 on the grounded sweep, and `max` over a dict returned
    # whichever came first: the rule this metric names changed between two
    # scorings of one run, and so did the denominator reported with it. A run
    # record whose numbers move when it is read again is not a record.
    tied = {"unsupported_claim": 1.0, "citation_unsupported": 1.0, "injection_suspected": 1.0}
    picked = {min(tied.items(), key=lambda kv: (-kv[1], kv[0]))[0] for _ in range(20)}
    assert picked == {"citation_unsupported"}, "the tie break is not by name"

    # And a genuinely worse rule still wins, whatever it is called.
    ranked = dict(tied, zzz_worse=1.0, aaa_better=0.5)
    assert min(ranked.items(), key=lambda kv: (-kv[1], kv[0]))[0] == "citation_unsupported"
    assert min({"aaa": 0.2, "zzz": 0.9}.items(), key=lambda kv: (-kv[1], kv[0]))[0] == "zzz"


def test_a_forbidden_value_the_verifier_caught_is_not_the_p0() -> None:
    # Doc 02 line 201 counts answers containing any forbidden value. Doc 07
    # line 233 counts one that survives verification and calls only that a P0.
    # The first live run was exactly the gap between them: one forbidden value,
    # on a card the Verifier flagged with thirteen flags and a confidence of
    # 0.25. Collapsing the two loses the thing that says where to work.
    assert harness.THRESHOLDS["forbidden_fact_rate"] == 0.0
    assert harness.THRESHOLDS["forbidden_fact_unflagged"] == 0.0
    assert "forbidden_fact_unflagged" in harness.LOWER_IS_BETTER

    caught = _metric("forbidden_fact_rate", 1, 12)
    p0 = _metric("forbidden_fact_unflagged", 0, 12)
    assert caught.verdict() == "fail", "a wrong value was written and that is still a failure"
    assert p0.verdict() == "pass", "the Verifier caught it, so the P0 did not happen"

    # And a value that does survive verification fails the P0 on one case,
    # because an absolute gate is not withheld for a small sample.
    assert _metric("forbidden_fact_unflagged", 1, 12).verdict() == "fail"
