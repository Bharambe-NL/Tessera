"""Compare two corpus trees byte for byte.

Doc 02 section 9 and doc 12 phase 3's acceptance: `gen build --seed 42` twice
yields identical ledgers. A corpus that drifts between builds makes doc 02
section 10.4's run to run diff meaningless, so this is checked rather than
assumed.
"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path


def digest_tree(root: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if path.is_file():
            key = path.relative_to(root).as_posix()
            out[key] = hashlib.sha256(path.read_bytes()).hexdigest()
    return out


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: compare_builds.py <first> <second>", file=sys.stderr)
        return 2

    a = digest_tree(Path(argv[0]))
    b = digest_tree(Path(argv[1]))

    only_a = sorted(set(a) - set(b))
    only_b = sorted(set(b) - set(a))
    differ = sorted(k for k in set(a) & set(b) if a[k] != b[k])

    print(f"files: {len(a)} vs {len(b)}")
    print(
        f"only in first: {len(only_a)}   only in second: {len(only_b)}   differing: {len(differ)}"
    )
    for k in differ[:12]:
        print(f"  differs: {k}")
    for k in (only_a + only_b)[:8]:
        print(f"  missing: {k}")

    identical = not (only_a or only_b or differ)
    print("\nIDENTICAL" if identical else "\nNOT IDENTICAL")
    return 0 if identical else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
