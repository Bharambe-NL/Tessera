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
from tessera_gen import questions as questions_mod
from tessera_gen import snapshots as snapshots_mod
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
