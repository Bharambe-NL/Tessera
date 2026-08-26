"""Score what the index retrieved. Doc 05 section 12.

The gates: local recall 0.90, regulatory 0.95, web 0.80. Doc 10 section 17
question 2 says the embedding model choice is settled "with the synthetic
recall numbers", and these are those numbers.

Recall here means the retriever's job and not the Synthesizer's: did a passage
containing the required fact come back at all. Whether the answer then states
it is doc 02 section 10.2's `fact_recall`, measured further down the pipeline
and against a different failure.

Scoring goes through `matchers.matches`, the same function every other metric
uses. A second matcher would be a second definition of a hit, and the two would
disagree the first time either changed.
"""

from __future__ import annotations

import json
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

from . import matchers

#: Doc 05 section 12.
THRESHOLDS = {"regulatory": 0.95, "local": 0.90, "web": 0.80}

#: Which folder a required source belongs to, from its document id prefix.
PREFIXES = {"reg-": "regulatory", "int-": "local", "web-": "web"}


@dataclass
class Bucket:
    hits: int = 0
    total: int = 0
    #: Questions where nothing came back at all, which is a different failure
    #: from ranking the right passage too low.
    empty: int = 0
    misses: list[str] = field(default_factory=list)

    @property
    def recall(self) -> float | None:
        return self.hits / self.total if self.total else None


def folder_for(source_id: str) -> str | None:
    for prefix, folder in PREFIXES.items():
        if source_id.startswith(prefix):
            return folder
    return None


def score(results: Path, corpus: Path) -> dict:
    facts = {
        f["fact_id"]: f
        for f in (
            json.loads(line)
            for line in (corpus / "facts.jsonl").read_text(encoding="utf-8").splitlines()
            if line.strip()
        )
    }

    rows = [
        json.loads(line)
        for line in results.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]

    overall = Bucket()
    by_folder: dict[str, Bucket] = defaultdict(Bucket)
    at_k: dict[int, Bucket] = {k: Bucket() for k in (1, 3, 5, 12)}

    for row in rows:
        passages = row.get("passages") or []
        text_at = [p.get("text", "") for p in passages]

        for fact_id in row.get("required_facts") or []:
            fact = facts.get(fact_id)
            if fact is None:
                continue

            # A required fact belongs to whichever folder the question's
            # required source lives in. Doc 05 section 12 states its gates per
            # retriever, so a single overall number would hide which one failed.
            folders = {f for f in (folder_for(s) for s in row.get("required_sources") or []) if f}

            found = any(matchers.matches(fact["kind"], fact["value"], t) for t in text_at)

            overall.total += 1
            overall.hits += int(found)
            if not passages:
                overall.empty += 1
            if not found and len(overall.misses) < 40:
                overall.misses.append(f"{row['q_id']} {fact_id} {row['text'][:60]}")

            for folder in folders:
                bucket = by_folder[folder]
                bucket.total += 1
                bucket.hits += int(found)
                if not passages:
                    bucket.empty += 1

            for k, bucket in at_k.items():
                bucket.total += 1
                bucket.hits += int(
                    any(matchers.matches(fact["kind"], fact["value"], t) for t in text_at[:k])
                )

    return {
        "matchers_version": matchers.MATCHERS_VERSION,
        "questions": len(rows),
        "overall": {
            "recall": overall.recall,
            "hits": overall.hits,
            "total": overall.total,
            "questions_with_no_passages": overall.empty,
        },
        "by_folder": {
            folder: {
                "recall": bucket.recall,
                "hits": bucket.hits,
                "total": bucket.total,
                "threshold": THRESHOLDS.get(folder),
                "verdict": verdict(bucket.recall, THRESHOLDS.get(folder)),
            }
            for folder, bucket in sorted(by_folder.items())
        },
        "at_k": {
            str(k): {"recall": bucket.recall, "hits": bucket.hits, "total": bucket.total}
            for k, bucket in sorted(at_k.items())
        },
        "sample_misses": overall.misses[:20],
    }


def verdict(recall: float | None, threshold: float | None) -> str:
    if recall is None:
        return "n/a"
    if threshold is None:
        return "reported"
    return "pass" if recall >= threshold else "fail"


def render(report: dict) -> str:
    lines = [
        "# Retrieval recall",
        "",
        f"Matchers {report['matchers_version']}. {report['questions']} questions.",
        "",
        "| Scope | Recall | Threshold | Verdict | Hits |",
        "| --- | --- | --- | --- | --- |",
    ]
    o = report["overall"]
    shown = f"{o['recall']:.3f}" if o["recall"] is not None else "n/a"
    lines.append(f"| overall | {shown} |  | reported | {o['hits']}/{o['total']} |")
    for folder, b in report["by_folder"].items():
        shown = f"{b['recall']:.3f}" if b["recall"] is not None else "n/a"
        lines.append(
            f"| {folder} | {shown} | {b['threshold']} | {b['verdict']} | {b['hits']}/{b['total']} |"
        )

    lines += ["", "## Recall at k", "", "| k | Recall |", "| --- | --- |"]
    for k, b in report["at_k"].items():
        shown = f"{b['recall']:.3f}" if b["recall"] is not None else "n/a"
        lines.append(f"| {k} | {shown} |")

    if o["questions_with_no_passages"]:
        lines += [
            "",
            f"{o['questions_with_no_passages']} fact lookups had no passages at all, "
            "which is a different failure from ranking the right one too low.",
        ]

    failed = [f for f, b in report["by_folder"].items() if b["verdict"] == "fail"]
    lines += ["", "## Verdict", ""]
    lines.append(
        "every gate met" if not failed else f"{len(failed)} below threshold: {', '.join(failed)}"
    )
    return "\n".join(lines) + "\n"
