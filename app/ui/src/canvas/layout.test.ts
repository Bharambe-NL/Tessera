/**
 * Layout is pure because it takes a height lookup rather than reading the DOM,
 * and this is the test that spends that. Each case fixes the heights, so the
 * position a card lands at is arithmetic with one right answer rather than
 * whatever the browser measured that run.
 */

import { describe, expect, it } from 'vitest';

import { BRANCH_X, CARD_W, GAP_Y, layout } from './layout.js';
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

describe('layout', () => {
  it('lays out nothing when the board is empty', () => {
    const cards: Card[] = [];
    expect(() => {
      layout(cards, () => 200);
    }).not.toThrow();
    expect(cards).toHaveLength(0);
  });

  it('keeps two roots clear of each other', () => {
    const a = card('a');
    const b = card('b');
    layout([a, b], () => 200);

    // Roots share the top edge and are separated across the row, so the gap
    // that has to hold is horizontal: a full card plus the branch column the
    // first root reserves whether or not it has branches yet.
    expect(a.position.y).toBe(0);
    expect(b.position.y).toBe(0);
    expect(b.position.x - a.position.x).toBe(CARD_W + BRANCH_X);
    expect(b.position.x).toBeGreaterThanOrEqual(a.position.x + CARD_W);
  });

  it('drops a follow-up below its parent by the parent measured height', () => {
    const root = card('root');
    const follow = card('follow', 'root', 'follow');
    // The lookup is what proves the injection: a taller parent must push its
    // follow-up further down, and nothing here has touched the DOM.
    layout([root, follow], (id) => (id === 'root' ? 500 : 200));

    expect(follow.position.x).toBe(root.position.x);
    expect(follow.position.y).toBe(root.position.y + 500 + GAP_Y);

    const shorter = card('root');
    const under = card('follow', 'root', 'follow');
    layout([shorter, under], (id) => (id === 'root' ? 250 : 200));
    expect(under.position.y).toBe(shorter.position.y + 250 + GAP_Y);
  });

  it('puts a branch to the right of its parent', () => {
    const root = card('root');
    const branch = card('branch', 'root', 'branch');
    layout([root, branch], () => 200);

    expect(branch.position.x).toBe(root.position.x + CARD_W + BRANCH_X);
    expect(branch.position.y).toBe(root.position.y);
  });

  it('leaves a pinned card where it was dropped', () => {
    const root = card('root');
    const pinned = card('pinned', 'root', 'follow');
    pinned.position = { x: 900, y: 40, dx: 0, dy: 0, pinned: true };
    layout([root, pinned], () => 200);

    expect(pinned.position.x).toBe(900);
    expect(pinned.position.y).toBe(40);
  });
});
