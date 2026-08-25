"""Tessera's synthetic corpus generator and eval substrate. Doc 02.

Synthetic first, because the Verifier's job is to catch unsupported claims, wrong
citations, stale sources and advice language, and none of those can be measured
on a real corpus without a human labelling every claim.
"""

from .rng import GENERATOR_VERSION

__all__ = ["GENERATOR_VERSION"]
