/**
 * Card layout.
 *
 * Ported from the prototype (`canvas-prototype.html:524`-547), whose constants
 * doc 11 section 3 adopts as tested. Follow-ups go below a card in a row;
 * branches go to the right, stacked. A card the user dragged keeps its offset
 * from the layout slot.
 *
 * The port changes three things:
 *   - it takes a height lookup rather than reading the DOM directly, so layout
 *     is pure and testable;
 *   - `parentId`/`kind` read from the doc 01 field names;
 *   - `pinned` lives inside `position` per doc 01 section 4.2.
 */

import type { Card } from './types.js';

export const CARD_W = 440;
export const GAP_Y = 90;
export const GAP_X = 60;
export const BRANCH_X = 120;
export const BRANCH_GAP = 40;

/** Fallback used before a card has been measured. The prototype's value. */
export const DEFAULT_CARD_H = 320;

export interface Box {
  w: number;
  h: number;
}

/** Measured card heights by card id. Missing entries fall back to DEFAULT_CARD_H. */
export type HeightLookup = (cardId: string) => number;

interface Index {
  branches: Map<string, Card[]>;
  follows: Map<string, Card[]>;
}

function indexChildren(cards: Card[]): Index {
  const branches = new Map<string, Card[]>();
  const follows = new Map<string, Card[]>();
  for (const c of cards) {
    if (c.parent_card_id === null) continue;
    // `read` and `exercise` cards attach like follow-ups.
    const bucket = c.kind === 'branch' ? branches : follows;
    const list = bucket.get(c.parent_card_id);
    if (list) list.push(c);
    else bucket.set(c.parent_card_id, [c]);
  }
  return { branches, follows };
}

function layoutSub(card: Card, x: number, y: number, idx: Index, h: HeightLookup): Box {
  // A pinned card holds where it was dropped, and its subtree lays out beneath
  // that rather than beneath the slot it would otherwise have had. Without this
  // the check belongs to roots alone: `layout` skips a pinned root, but a
  // parent's pass walks into its branches and follow-ups and writes their
  // positions anyway, so pinning a child looked like it worked until the next
  // relayout put the card back.
  //
  // The box this returns is still the subtree's own size, so siblings keep the
  // spacing they would have had. A pinned card is out of the flow by choice,
  // and reflowing its siblings around it would move cards the person did not
  // touch.
  if (card.position.pinned) {
    x = card.position.x - card.position.dx;
    y = card.position.y - card.position.dy;
  }
  card.position.x = x + card.position.dx;
  card.position.y = y + card.position.dy;

  const own = h(card.id);
  const branches = idx.branches.get(card.id) ?? [];
  const follows = idx.follows.get(card.id) ?? [];

  // Branches stack down the right hand column.
  let branchW = 0;
  let branchY = y;
  let branchH = 0;
  for (const b of branches) {
    const box = layoutSub(b, x + CARD_W + BRANCH_X, branchY, idx, h);
    branchW = Math.max(branchW, box.w);
    branchY += box.h + BRANCH_GAP;
    branchH = branchY - y - BRANCH_GAP;
  }
  const rightW = branches.length ? BRANCH_X + branchW : 0;

  // Follow-ups flow left to right on the row below whichever is taller,
  // this card or its stack of branches.
  let followX = x;
  let followH = 0;
  let followW = 0;
  const followY = y + Math.max(own, branchH) + GAP_Y;
  for (const f of follows) {
    const box = layoutSub(f, followX, followY, idx, h);
    followX += box.w + GAP_X;
    followH = Math.max(followH, box.h);
    followW = followX - x - GAP_X;
  }

  return {
    w: Math.max(CARD_W + rightW, followW),
    h: follows.length ? followY - y + followH : Math.max(own, branchH),
  };
}

/**
 * Position every card on the board in place. Roots lay out left to right;
 * a pinned root keeps its position and only its subtree is re-laid.
 */
export function layout(cards: Card[], heightOf: HeightLookup = () => DEFAULT_CARD_H): void {
  const idx = indexChildren(cards);

  // Every root lays out in order, the pinned ones included. `layoutSub` leaves a
  // pinned card where it was dropped, and the cursor still advances by the width
  // that card would have had, so its slot stays reserved.
  //
  // Skipping a pinned root instead, which is what this did, freed its slot and
  // pulled every root after it to the left. Dragging one card moved the cards
  // beside it, which is the complaint the drag exists to answer, in miniature.
  // The second pass this replaces is gone with it: a pinned card is now handled
  // wherever the walk reaches it, so a pinned branch holds too.
  let x = 0;
  for (const root of cards) {
    if (root.parent_card_id !== null) continue;
    const box = layoutSub(root, x, 0, idx, heightOf);
    x += box.w + BRANCH_X;
  }
}

/** Bounding box of every card, used by "fit to view". */
export function boundsOf(cards: Card[], heightOf: HeightLookup = () => DEFAULT_CARD_H) {
  if (cards.length === 0) return { x: 0, y: 0, w: CARD_W, h: DEFAULT_CARD_H };
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const c of cards) {
    minX = Math.min(minX, c.position.x);
    minY = Math.min(minY, c.position.y);
    maxX = Math.max(maxX, c.position.x + CARD_W);
    maxY = Math.max(maxY, c.position.y + heightOf(c.id));
  }
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}
