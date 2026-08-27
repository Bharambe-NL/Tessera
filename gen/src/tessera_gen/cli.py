"""The generator CLI. Doc 02 section 11.

``gen build --seed 42 --out synthetic/``, ``gen verify``, ``gen snapshot T2``,
``gen serve`` for the local web corpus.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import boards as boards_mod
from . import breadth as breadth_mod
from . import corpus as corpus_mod
from . import edge_cases, harness, mess, retrieval, writer
from . import memory as memory_mod
from . import questions as questions_mod
from . import snapshots as snapshots_mod
from . import vault as vault_mod
from .facts import generate_facts
from .rng import GENERATOR_VERSION

DEFAULT_SEED = 42
DEFAULT_OUT = Path("synthetic")


def build(seed: int, out: Path, snapshot: str | None = None) -> dict:
    """Everything, in dependency order. Doc 02 section 2: a corpus, then a
    question set, then boards, because each depends on the previous one.

    With a snapshot label, the corpus is written as it stands at that time
    rather than as the union of every time, and it goes to its own root so the
    two can sit side by side. Doc 02 section 5.4 wants a board written at T1 and
    reopened at T3, which needs both trees on disk at once.
    """
    root = out / (f"{seed}-{snapshot}" if snapshot else str(seed))

    facts = generate_facts(seed)

    documents = corpus_mod.build_layer_one(seed, facts)
    documents.extend(edge_cases.build_layer_two(seed, facts))
    _, transformations = mess.apply(seed, documents)

    # Plantings are recorded once, after every layer has contributed, so a fact
    # knows every document that carries it including the messy copies.
    corpus_mod.record_plantings(documents, facts)
    problems = corpus_mod.verify_exact_plantings(documents, facts)

    question_set, dropped = questions_mod.generate(seed, facts, documents)
    breadth_set = breadth_mod.generate(seed)
    board_set = boards_mod.generate(seed, facts, documents, question_set)

    # Doc 16 section 5's synthetic vault, after boards because two dozen of its
    # pages are saved from cards. Its facts join the ledger and its questions
    # join the set, so a page-only answer is scored by the same matchers as any
    # other rather than by a second set of rules.
    vault_truth = vault_mod.generate(seed, facts, documents, question_set, board_set)
    # The facts join the ledger, so a page-only answer is scored by the same
    # matchers as any other. The questions do not join the question set: doc 02
    # section 6 fixes that set at 400 with a stated shape, and the vault's own
    # families are a second set beside it, as the breadth questions already are.
    facts.extend(vault_truth.facts)

    # Memory runs after boards because it plants cards on them.
    memory_truth = memory_mod.build(seed, facts, documents, question_set, board_set)
    snaps = snapshots_mod.build(seed, documents, facts)

    # A snapshot tree holds the files that exist at its label, so a question
    # whose source was deleted or taken down by then cannot be answered from it.
    # Doc 02 section 5.4 means that to happen; recording which questions it hits
    # keeps a later sweep from reading the gap as a retrieval failure.
    stranded: list[dict] = []
    if snapshot:
        documents = snapshots_mod.materialise(
            snapshot, documents, snapshots_mod.plan(seed, documents)
        )
        present = {d.doc_id for d in documents}
        stranded = [
            {
                "q_id": q.q_id,
                "snapshot": snapshot,
                "missing_sources": sorted(set(q.required_sources) - present),
            }
            for q in question_set
            if set(q.required_sources) - present
        ]

    return writer.write_corpus(
        root=root,
        seed=seed,
        facts=facts,
        documents=documents,
        questions=question_set,
        breadth=breadth_set,
        boards=board_set,
        vault=vault_truth,
        snapshots=snaps,
        memory_truth=memory_truth,
        transformations=transformations,
        dropped_questions=dropped,
        verification_problems=problems,
        snapshot=snapshot,
        stranded_questions=stranded,
    )


def corpus_root(out: Path, seed: int, snapshot: str | None = None) -> Path:
    """Where a corpus lives. A snapshot tree sits beside the default one, so
    both can be on disk at once and a T1 board can be reopened at T3."""
    return out / (f"{seed}-{snapshot}" if snapshot else str(seed))


def verify(seed: int, out: Path, snapshot: str | None = None) -> int:
    """Re-run the checks that make a corpus usable, against what is on disk."""
    root = corpus_root(out, seed, snapshot)
    ledger = root / "ledger.jsonl"
    if not ledger.exists():
        flag = f" --snapshot {snapshot}" if snapshot else ""
        print(f"no corpus at {root}. Run `gen build --seed {seed}{flag}` first.", file=sys.stderr)
        return 2

    rows = [json.loads(line) for line in ledger.read_text(encoding="utf-8").splitlines() if line]
    build_row = next((r for r in rows if r.get("type") == "build"), None)
    problems = [r for r in rows if r.get("type") == "verification_problem"]
    dropped = [r for r in rows if r.get("type") == "dropped_question"]
    stranded = [r for r in rows if r.get("type") == "stranded_question"]

    print(f"corpus {build_row['corpus_name'] if build_row else '?'} at {root}")
    print(f"  facts        {build_row['facts']['total'] if build_row else '?'}")
    print(f"  documents    {build_row['documents']['total'] if build_row else '?'}")
    print(f"  questions    {build_row['questions']['total'] if build_row else '?'}")
    print(f"  boards       {build_row['boards']['total'] if build_row else '?'}")
    print(f"  pages        {build_row.get('vault', {}).get('total', '?') if build_row else '?'}")
    print(f"  plantings    {sum(1 for r in rows if r.get('type') == 'planting')}")
    print(f"  dropped      {len(dropped)}")
    if stranded:
        print(f"  stranded     {len(stranded)} questions whose sources this label no longer holds")
    print(f"  problems     {len(problems)}")

    for p in problems[:10]:
        print(f"    {p['detail']}")
    if len(problems) > 10:
        print(f"    and {len(problems) - 10} more")

    # A dropped question is expected and logged; a verification problem is not.
    return 1 if problems else 0


def snapshot(seed: int, out: Path, label: str) -> int:
    path = out / str(seed) / "snapshots" / f"{label}.json"
    if not path.exists():
        print(f"no snapshot {label} at {path}", file=sys.stderr)
        return 2
    data = json.loads(path.read_text(encoding="utf-8"))
    print(
        f"{data['label']} at {data['at']}: {len(data['files'])} files, "
        f"{len(data['facts_in_force'])} facts in force"
    )
    for note in data.get("notes", []):
        print(f"  {note}")
    changes: dict[str, int] = {}
    for f in data["files"]:
        if f.get("change"):
            changes[f["change"]] = changes.get(f["change"], 0) + 1
    for kind, count in sorted(changes.items()):
        print(f"  {count} {kind}")
    return 0


def score(results: Path, corpus: Path) -> int:
    """Turn a run record into the metrics. Doc 02 sections 10.2 to 10.4."""
    if not (results / "runs.jsonl").exists():
        print(f"no run record at {results}", file=sys.stderr)
        return 2
    if not (corpus / "facts.jsonl").exists():
        print(f"no corpus at {corpus}", file=sys.stderr)
        return 2

    report = harness.score(results, corpus)
    previous = harness.find_previous(results)
    harness.write_report(results, report, previous)

    print(harness.render(report, previous))
    # A metric below its threshold fails the run, which is what makes this
    # usable as a gate rather than a readout.
    return 1 if report.failed else 0


def score_retrieval(results: Path, corpus: Path) -> int:
    """Doc 05 section 12's recall gates, measured on the index alone."""
    if not results.exists():
        print(f"no results at {results}", file=sys.stderr)
        return 2
    if not (corpus / "facts.jsonl").exists():
        print(f"no corpus at {corpus}. Run `gen build` first.", file=sys.stderr)
        return 2

    report = retrieval.score(results, corpus)
    print(retrieval.render(report))
    (results.parent / f"{results.stem}.report.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    failed = [f for f, b in report["by_folder"].items() if b["verdict"] == "fail"]
    return 1 if failed else 0


def serve(seed: int, out: Path, port: int) -> int:
    """The local static server for the synthetic web. Doc 02 section 10.1 points
    the web retriever at it, so nothing in evaluation ever leaves the machine."""
    import functools
    import http.server
    import socketserver

    web_root = out / str(seed) / "corpus" / "web"
    if not web_root.exists():
        print(f"no web corpus at {web_root}. Run `gen build --seed {seed}` first.", file=sys.stderr)
        return 2

    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(web_root))
    with socketserver.TCPServer(("127.0.0.1", port), handler) as httpd:
        print(f"serving {web_root} on http://127.0.0.1:{port}")
        print("each directory is one synthetic site; every domain ends in .invalid")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nstopped")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="gen", description="Tessera synthetic corpus generator")
    parser.add_argument("--version", action="version", version=GENERATOR_VERSION)
    sub = parser.add_subparsers(dest="command", required=True)

    p_build = sub.add_parser("build", help="generate a corpus")
    p_build.add_argument("--seed", type=int, default=DEFAULT_SEED)
    p_build.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p_build.add_argument(
        "--snapshot",
        choices=list(snapshots_mod.TIMELINE),
        default=None,
        help="write the corpus as it stands at this label, to <out>/<seed>-<label>",
    )

    p_verify = sub.add_parser("verify", help="check a corpus on disk")
    p_verify.add_argument("--seed", type=int, default=DEFAULT_SEED)
    p_verify.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p_verify.add_argument("--snapshot", choices=list(snapshots_mod.TIMELINE), default=None)

    p_snapshot = sub.add_parser("snapshot", help="describe one snapshot")
    p_snapshot.add_argument("label", choices=list(snapshots_mod.TIMELINE))
    p_snapshot.add_argument("--seed", type=int, default=DEFAULT_SEED)
    p_snapshot.add_argument("--out", type=Path, default=DEFAULT_OUT)

    p_score = sub.add_parser("score", help="score a run record against the corpus")
    p_score.add_argument("--results", type=Path, required=True)
    p_score.add_argument("--seed", type=int, default=DEFAULT_SEED)
    p_score.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p_score.add_argument(
        "--snapshot",
        choices=list(snapshots_mod.TIMELINE),
        default=None,
        help="score against the corpus as it stands at this label",
    )

    p_recall = sub.add_parser(
        "score-retrieval", help="score what the index retrieved, doc 05 section 12"
    )
    p_recall.add_argument("--results", type=Path, required=True)
    p_recall.add_argument("--seed", type=int, default=DEFAULT_SEED)
    p_recall.add_argument("--out", type=Path, default=DEFAULT_OUT)

    p_serve = sub.add_parser("serve", help="serve the synthetic web corpus")
    p_serve.add_argument("--seed", type=int, default=DEFAULT_SEED)
    p_serve.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p_serve.add_argument("--port", type=int, default=8731)

    args = parser.parse_args(argv)

    if args.command == "build":
        summary = build(args.seed, args.out, args.snapshot)
        print(json.dumps(summary, indent=2, sort_keys=True))
        return 1 if summary["verification_problems"] else 0
    if args.command == "verify":
        return verify(args.seed, args.out, args.snapshot)
    if args.command == "snapshot":
        return snapshot(args.seed, args.out, args.label)
    if args.command == "score":
        return score(args.results, corpus_root(args.out, args.seed, args.snapshot))
    if args.command == "score-retrieval":
        return score_retrieval(args.results, args.out / str(args.seed))
    if args.command == "serve":
        return serve(args.seed, args.out, args.port)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
