/**
 * Threads are a reading of the read model, so the grouping is pure and these
 * cases pin it: which cards are tiles, which are turns, and which tile a turn
 * is drawn in.
 */

import { describe, expect, it } from 'vitest';

import { isTurn, threads } from './thread.js';
import type { Card, CardKind } from './types.js';

function card(id: string, parent: string | null = null, kind: CardKind = 'root'): Card {
  return {
    id,
    parent_card_id: parent,
    kind,
    anchor_text: null,
    anchor_block_ref: null,
    question: `question ${id}`,
    depth: 'fast',
    audience_id: null,
    answer: null,
    findings: [],
    visual: null,
    citations: [],
    flags: [],
    status: 'done',
    confidence: null,
    model_alias: null,
    stages: [],
    position: { x: 0, y: 0, dx: 0, dy: 0, pinned: false },
  };
}

describe('threads', () => {
  it('folds a chain of follow-ups into the tile it started from', () => {
    const cards = [card('a'), card('b', 'a', 'follow'), card('c', 'b', 'follow')];
    const t = threads(cards);
    expect(t.tiles.map((c) => c.id)).toEqual(['a']);
    expect(t.turns.get('a')?.map((c) => c.id)).toEqual(['b', 'c']);
    expect(t.headOf('c')).toBe('a');
    expect(t.headOf('a')).toBe('a');
  });

  it('keeps branches, reads and exercises as tiles of their own', () => {
    const cards = [
      card('a'),
      card('b', 'a', 'branch'),
      card('r', 'a', 'read'),
      card('x', 'a', 'exercise'),
      card('f', 'b', 'follow'),
    ];
    const t = threads(cards);
    expect(t.tiles.map((c) => c.id)).toEqual(['a', 'b', 'r', 'x']);
    // A follow-up asked in a branch is a turn in the branch.
    expect(t.turns.get('b')?.map((c) => c.id)).toEqual(['f']);
    expect(t.turns.has('a')).toBe(false);
  });

  it('draws a follow-up whose parent is not on the board as a tile', () => {
    const orphan = card('f', 'gone', 'follow');
    const t = threads([orphan]);
    expect(t.tiles).toEqual([orphan]);
    expect(isTurn(orphan, new Map())).toBe(false);
  });

  it('keeps a card caught in a parent loop on the board as a tile', () => {
    const cards = [card('a', 'b', 'follow'), card('b', 'a', 'follow')];
    expect(threads(cards).tiles.map((c) => c.id)).toEqual(['a', 'b']);
  });
});
