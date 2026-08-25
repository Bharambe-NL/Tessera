"""Fact matching tolerance. Doc 02 section 11.

"Fact matching tolerance rules live in `matchers.py` and are versioned with the
generator: numeric equality with unit normalisation, date equality across
formats, definition match by required key phrases listed in the fact."

Versioned deliberately. Doc 02 section 10.4 diffs one harness run against the
previous one for the same corpus and policy, and a silent change to what counts
as a match would move every number in that diff without anything about the agents
having changed.
"""

from __future__ import annotations

import re

#: Bumped when a change here would move a metric at an unchanged corpus.
MATCHERS_VERSION = "1.0"

#: Units that mean the same thing, normalised to the first form.
UNIT_ALIASES = {
    "%": ["%", "percent", "per cent", "pct"],
    "million EUR": ["million eur", "eur million", "m eur", "€m", "million euro"],
    "thousand EUR": ["thousand eur", "eur thousand", "k eur", "€k", "thousand euro"],
    "EUR": ["eur", "euro", "€"],
    "days": ["days", "day", "calendar days"],
    "business days": ["business days", "working days"],
    "hours": ["hours", "hour", "hrs"],
    "months": ["months", "month"],
    "exceptions per year": ["exceptions per year", "exceptions a year", "annual exceptions"],
    "% of own funds": ["% of own funds", "percent of own funds"],
}

_UNIT_LOOKUP = {
    alias: canonical for canonical, aliases in UNIT_ALIASES.items() for alias in aliases
}

_NUMBER = re.compile(r"-?\d[\d,]*\.?\d*")


def normalise_unit(unit: str) -> str:
    return _UNIT_LOOKUP.get(unit.strip().lower(), unit.strip().lower())


def normalise_number(text: str) -> float | None:
    """The first number in a span, with thousands separators removed."""
    m = _NUMBER.search(text)
    if not m:
        return None
    try:
        return float(m.group(0).replace(",", ""))
    except ValueError:
        return None


def numbers_in(text: str) -> list[float]:
    out = []
    for m in _NUMBER.finditer(text):
        try:
            out.append(float(m.group(0).replace(",", "")))
        except ValueError:
            continue
    return out


def numeric_match(expected: dict, text: str) -> bool:
    """Doc 02 section 11: numeric equality with unit normalisation.

    The unit has to be present *somewhere* in the span rather than immediately
    after the number, because "8 percent of risk weighted assets" and "a
    requirement of 8, expressed as a percentage" both state the same fact.
    """
    amount = expected.get("amount")
    if amount is None:
        return False
    try:
        target = float(str(amount).replace(",", ""))
    except ValueError:
        return False

    if not any(abs(n - target) < 1e-9 for n in numbers_in(text)):
        return False

    unit = expected.get("unit")
    if not unit:
        return True
    canonical = normalise_unit(unit)
    lowered = text.lower()
    return any(alias in lowered for alias in UNIT_ALIASES.get(canonical, [canonical]))


#: The date forms a source might reasonably write.
_DATE_PATTERNS = (
    re.compile(r"(\d{4})-(\d{2})-(\d{2})"),
    re.compile(r"(\d{1,2})/(\d{1,2})/(\d{4})"),
)

_MONTHS = {
    "january": 1,
    "february": 2,
    "march": 3,
    "april": 4,
    "may": 5,
    "june": 6,
    "july": 7,
    "august": 8,
    "september": 9,
    "october": 10,
    "november": 11,
    "december": 12,
}


def dates_in(text: str) -> list[tuple[int, int, int]]:
    out: list[tuple[int, int, int]] = []
    for m in _DATE_PATTERNS[0].finditer(text):
        out.append((int(m.group(1)), int(m.group(2)), int(m.group(3))))
    for m in _DATE_PATTERNS[1].finditer(text):
        out.append((int(m.group(3)), int(m.group(2)), int(m.group(1))))
    # "1 March 2026" and "March 1, 2026".
    for m in re.finditer(r"(\d{1,2})\s+([A-Za-z]+)\s+(\d{4})", text):
        month = _MONTHS.get(m.group(2).lower())
        if month:
            out.append((int(m.group(3)), month, int(m.group(1))))
    for m in re.finditer(r"([A-Za-z]+)\s+(\d{1,2}),?\s+(\d{4})", text):
        month = _MONTHS.get(m.group(1).lower())
        if month:
            out.append((int(m.group(3)), month, int(m.group(2))))
    return out


def date_match(expected: dict, text: str) -> bool:
    """Doc 02 section 11: date equality across formats."""
    raw = expected.get("date")
    if not raw:
        return False
    try:
        year, month, day = (int(p) for p in str(raw).split("-"))
    except ValueError:
        return False
    return (year, month, day) in dates_in(text)


def phrase_match(expected: dict, text: str, required: float = 0.6) -> bool:
    """Doc 02 section 11: definition match by required key phrases.

    A definition restated in the reader's own words is still the definition, so
    the test is coverage of the phrases the fact declared rather than string
    equality on a sentence nobody would reproduce exactly.
    """
    phrases = [p for p in expected.get("key_phrases", []) if p]
    if not phrases:
        body = str(expected.get("text", "")).strip().lower()
        return bool(body) and body in text.lower()

    lowered = text.lower()
    hits = sum(1 for p in phrases if str(p).lower() in lowered)
    return hits / len(phrases) >= required


def matches(fact_kind: str, value: dict, text: str) -> bool:
    """Whether a span states this fact's value, under the rules for its kind."""
    if fact_kind == "number":
        return numeric_match(value, text)
    if fact_kind == "date":
        return date_match(value, text)
    return phrase_match(value, text)


def states_any(facts_values: list[tuple[str, str, dict]], text: str) -> list[str]:
    """Which of a set of facts a span states. Returns the fact ids.

    Used by the forbidden fact rate, which asks whether an answer contains any
    value it was not supposed to (doc 02 section 10.2).
    """
    return [fact_id for fact_id, kind, value in facts_values if matches(kind, value, text)]
