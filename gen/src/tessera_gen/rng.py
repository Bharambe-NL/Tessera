"""Seeded randomness, with one rule.

Doc 02 section 9: "One seed drives everything: entity names, fact values, which
paragraphs plant which facts, the mess transformations, the snapshot timeline."

The rule is that every stage draws from its *own* stream, derived from the master
seed and the stage's name. A single shared stream would mean adding a stage, or
drawing one extra value inside one, shifts every later draw and changes a corpus
that nothing about the change should have touched. Doc 02 section 10.4 compares
runs against each other, so a corpus that moves under an unrelated edit makes
every diff unreadable.
"""

from __future__ import annotations

import hashlib
import random
from collections.abc import Iterable, Sequence
from typing import TypeVar

T = TypeVar("T")

#: Bumped when a change to this module would alter a corpus at the same seed.
#: Doc 02 section 9 names a corpus `<generator_version>-<seed>`.
GENERATOR_VERSION = "0.1.0"


def stream(seed: int, *stage: str) -> random.Random:
    """A stream for one named stage, independent of every other stage."""
    label = "/".join(stage)
    digest = hashlib.sha256(f"{seed}:{label}".encode()).digest()
    return random.Random(int.from_bytes(digest[:8], "big"))


class Rng:
    """A named stream with the helpers the generator actually uses."""

    def __init__(self, seed: int, *stage: str) -> None:
        self.seed = seed
        self.stage = stage
        self._r = stream(seed, *stage)

    def derive(self, *stage: str) -> Rng:
        """A child stream, so a per document draw cannot disturb its siblings."""
        return Rng(self.seed, *self.stage, *stage)

    def choice(self, items: Sequence[T]) -> T:
        return self._r.choice(list(items))

    def sample(self, items: Sequence[T], k: int) -> list[T]:
        pool = list(items)
        k = min(k, len(pool))
        return self._r.sample(pool, k)

    def shuffled(self, items: Iterable[T]) -> list[T]:
        pool = list(items)
        self._r.shuffle(pool)
        return pool

    def randint(self, low: int, high: int) -> int:
        return self._r.randint(low, high)

    def chance(self, probability: float) -> bool:
        return self._r.random() < probability

    def random(self) -> float:
        return self._r.random()

    def weighted(self, options: Sequence[tuple[T, float]]) -> T:
        """Pick by weight. Used for the fact kind mix in doc 02 section 3."""
        total = sum(w for _, w in options)
        cut = self._r.random() * total
        upto = 0.0
        for value, weight in options:
            upto += weight
            if cut <= upto:
                return value
        return options[-1][0]
