"""Layer 3, realistic mess. Doc 02 section 5.3.

"Layer 1 and 2 documents are too clean. Layer 3 applies transformations that
mimic real folders."

Every transformation is deterministic from the seed and logged per document, so a
failure can be traced to the exact transformation that caused it. That last part
is the reason the log exists: without it, a recall number that drops after adding
OCR noise tells you nothing about which of eight transformations did it.
"""

from __future__ import annotations

from dataclasses import dataclass

from .corpus import Document, Passage
from .rng import Rng

#: Doc 02 section 5.3's shares.
OCR_NOISE_SHARE = 0.20
SCANNED_SHARE = 0.05
DUPLICATE_SHARE = 0.08
LONG_DOCUMENT_COUNT = 2
BROKEN_FILE_COUNT = 3

#: Where the planted fact sits in the 300 page document.
LONG_DOCUMENT_PAGES = 300
LONG_DOCUMENT_FACT_PAGE = 212

#: Characters OCR reliably confuses. A substitution rate high enough to matter
#: and low enough that a human could still read the page.
OCR_SUBSTITUTIONS = {"0": "O", "1": "l", "5": "S", "8": "B", "rn": "m", "cl": "d"}
OCR_RATE = 0.06


@dataclass
class Transformation:
    doc_id: str
    kind: str
    detail: str


def apply(seed: int, documents: list[Document]) -> tuple[list[Document], list[Transformation]]:
    """Transform the corpus in place and return the new documents plus the log."""
    log: list[Transformation] = []
    added: list[Document] = []

    pdf_like = [d for d in documents if d.format in ("pdf", "docx")]
    rng = Rng(seed, "mess")

    # --- OCR noise on a fifth of the scanned-able documents -----------------
    for doc in _share(rng.derive("ocr"), pdf_like, OCR_NOISE_SHARE):
        for passage in doc.passages:
            passage.text = _ocr_noise(
                rng.derive("ocr", doc.doc_id, passage.passage_id), passage.text
            )
        doc.transformations.append("ocr_noise")
        log.append(Transformation(doc.doc_id, "ocr_noise", f"rate {OCR_RATE}"))

    # --- Scanned pages with no text layer ----------------------------------
    # Doc 02 section 5.3: exercises the Reader's vision path inside retrieval,
    # and doc 05 section 12 sets scanned pdf recall at 0.70.
    for doc in _share(rng.derive("scanned"), pdf_like, SCANNED_SHARE):
        doc.format = "pdf"
        doc.transformations.append("scanned_no_text_layer")
        log.append(
            Transformation(
                doc.doc_id, "scanned_no_text_layer", "rendered to image, no extractable text"
            )
        )

    # --- Duplicate files with one paragraph changed ------------------------
    for doc in _share(rng.derive("duplicates"), documents, DUPLICATE_SHARE):
        if doc.kind != "internal":
            continue
        copy = _duplicate(rng.derive("duplicates", doc.doc_id), doc)
        added.append(copy)
        log.append(
            Transformation(
                copy.doc_id,
                "near_duplicate_file",
                f"copy of {doc.doc_id} with one paragraph changed",
            )
        )

    # --- Very long documents ------------------------------------------------
    for i in range(LONG_DOCUMENT_COUNT):
        source = rng.derive("long", str(i)).choice([d for d in documents if d.kind == "internal"])
        long_doc = _lengthen(rng.derive("long", str(i)), source, i)
        added.append(long_doc)
        log.append(
            Transformation(
                long_doc.doc_id,
                "very_long_document",
                f"{LONG_DOCUMENT_PAGES} pages, fact on page {LONG_DOCUMENT_FACT_PAGE}",
            )
        )

    # --- Files that are empty, corrupt, or password protected ---------------
    for i, mode in enumerate(("empty", "corrupt", "password_protected")[:BROKEN_FILE_COUNT]):
        broken = Document(
            doc_id=f"int-broken-{mode}",
            kind="internal",
            title=f"Unreadable file ({mode})",
            path=f"internal/Minutes/int-broken-{mode}." + ("pdf" if mode != "empty" else "md"),
            format="pdf" if mode != "empty" else "md",
            issuer="Meerkant Bank",
            published_at="2025-01-08",
        )
        broken.transformations.append(mode)
        # No passages: nothing is planted here, and nothing should be recalled
        # from it. Doc 05 section 10 `parse_error`: skip the file, record it in
        # the index errors, and carry on.
        added.append(broken)
        log.append(Transformation(broken.doc_id, mode, "must be skipped and reported, never fatal"))
        _ = i

    # --- A spreadsheet whose totals row does not sum ------------------------
    sheet = _bad_spreadsheet(rng.derive("spreadsheet"))
    added.append(sheet)
    log.append(
        Transformation(
            sheet.doc_id,
            "totals_row_does_not_sum",
            "merged headers, and the total is not the sum of the rows",
        )
    )

    documents.extend(added)
    return added, log


def _share(rng: Rng, documents: list[Document], share: float) -> list[Document]:
    if not documents:
        return []
    count = max(1, round(len(documents) * share))
    return rng.sample(documents, count)


def _ocr_noise(rng: Rng, text: str) -> str:
    """Character substitutions at a set rate, and lost line breaks.

    Values inside a planted passage are damaged along with everything else. That
    is the point: doc 02 section 10.3 reports scanned recall without a threshold
    precisely because some of it is unreadable.
    """
    out = []
    for ch in text:
        if ch in OCR_SUBSTITUTIONS and rng.chance(OCR_RATE):
            out.append(OCR_SUBSTITUTIONS[ch])
        else:
            out.append(ch)
    noisy = "".join(out)
    # Lost line breaks: paragraphs run together the way a bad scan produces.
    return noisy.replace("\n\n", "\n") if rng.chance(0.5) else noisy


def _duplicate(rng: Rng, doc: Document) -> Document:
    copy = Document(
        doc_id=f"{doc.doc_id}-copy",
        kind=doc.kind,
        title=f"{doc.title} (copy)",
        # A different name in a different folder, which is how these actually
        # appear: someone saved it somewhere else.
        path=doc.path.replace("/", "/archive/", 1).replace(doc.doc_id, f"{doc.doc_id}-copy"),
        format=doc.format,
        issuer=doc.issuer,
        published_at=doc.published_at,
        language=doc.language,
    )
    copy.transformations.append("near_duplicate_file")

    changed_index = rng.randint(0, max(len(doc.passages) - 1, 0))
    for i, passage in enumerate(doc.passages):
        text = passage.text
        plants = list(passage.plants)
        if i == changed_index:
            text = text + "\n\nNote: this paragraph was revised in the archived copy."
            # The changed paragraph no longer plants what it planted: a copy that
            # still claimed the planting would make dedupe look correct when it
            # had merged two different things.
            plants = []
        copy.passages.append(
            Passage(
                passage_id=passage.passage_id.replace(doc.doc_id, copy.doc_id),
                text=text,
                location=dict(passage.location),
                plants=plants,
            )
        )
    return copy


def _lengthen(rng: Rng, source: Document, index: int) -> Document:
    """A 300 page document whose planted fact is on page 212.

    Doc 02 section 5.3. This is the case that separates a retriever which chunks
    and ranks from one that reads the first page and stops.
    """
    doc = Document(
        doc_id=f"int-long-{index:02d}",
        kind="internal",
        title="Annual supervisory submission",
        path=f"internal/Minutes/int-long-{index:02d}.md",
        format="md",
        issuer=source.issuer,
        published_at="2025-10-01",
    )
    doc.transformations.append("very_long_document")

    planted = next((p for p in source.passages if p.plants), None)

    for page in range(1, LONG_DOCUMENT_PAGES + 1):
        if page == LONG_DOCUMENT_FACT_PAGE and planted is not None:
            text = f"## Page {page}\n\n{planted.text}"
            plants = list(planted.plants)
        else:
            text = (
                f"## Page {page}\n\n"
                f"{rng.derive(str(page)).choice(FILLER)} "
                f"No change is reported for this section in the current period."
            )
            plants = []
        doc.passages.append(
            Passage(
                passage_id=f"{doc.doc_id}-p{page:04d}",
                text=text,
                location={"page": page},
                plants=plants,
            )
        )
    return doc


FILLER = (
    "The position is carried forward from the previous submission.",
    "This section is included for completeness and contains no new material.",
    "Figures in this section are unchanged from the prior reporting period.",
    "The relevant control was tested and no exception was recorded.",
)


def _bad_spreadsheet(rng: Rng) -> Document:
    """Merged headers and a totals row that does not sum. Doc 02 section 5.3.

    The structured retriever reads this at M6, and the totals row is the trap:
    an answer that quotes the total as though it were the sum of the rows has
    stated a number no passage supports.
    """
    doc = Document(
        doc_id="int-exposures-01",
        kind="internal",
        title="Exposure schedule",
        path="internal/Risk/int-exposures-01.xlsx",
        format="xlsx",
        issuer="Meerkant Bank",
        published_at="2025-09-01",
    )
    doc.transformations.append("totals_row_does_not_sum")

    rows = [
        ("Trading book", rng.randint(100, 400)),
        ("Banking book", rng.randint(200, 900)),
        ("Off balance sheet", rng.randint(50, 200)),
    ]
    real_total = sum(v for _, v in rows)
    # Off by a visible margin, so a scorer can tell a quoted total from a sum.
    stated_total = real_total + rng.randint(11, 40)

    lines = ["| Book | Exposure (million EUR) |", "| --- | --- |"]
    lines += [f"| {name} | {value} |" for name, value in rows]
    lines.append(f"| Total | {stated_total} |")

    doc.passages.append(
        Passage(
            passage_id="int-exposures-01-p0001",
            text="\n".join(lines),
            location={"sheet": "Exposures", "rows": f"1-{len(rows) + 3}"},
        )
    )
    doc.passages.append(
        Passage(
            passage_id="int-exposures-01-p0002",
            text=(
                "The total row is carried over from the prior submission and has not been "
                "recalculated. It does not equal the sum of the rows above."
            ),
            location={"sheet": "Exposures", "rows": "note"},
        )
    )
    return doc
