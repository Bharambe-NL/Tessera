/**
 * Threads: a tile and the follow-ups asked in it.
 *
 * The owner, 2026-10-03: a follow-up asked in an open tile continues that
 * tile's discussion, so its answer belongs inside the tile, scrolled to, and a
 * new tile is for when a person asks to investigate something further. The
 * core still stores every follow-up as its own card with its own parent, run
 * and citations, so this is a reading of the read model and nothing more: a
 * card of kind `follow` whose parent is on the board renders as a turn inside
 * the tile its chain starts from. Branches, reads and exercises stay tiles.
 */

import type { Card } from './types.js';

export interface Threads {
  /** The cards drawn as tiles, in board order. */
  tiles: Card[];
  /** Each tile's follow-up turns, in board order, keyed by the tile's id. */
  turns: Map<string, Card[]>;
  /** The tile a card is drawn in: itself for a tile, its thread head for a turn. */
  headOf: (cardId: string) => string;
}

export function isTurn(card: Card, byId: Map<string, Card>): boolean {
  return card.kind === 'follow' && card.parent_card_id !== null && byId.has(card.parent_card_id);
}

export function threads(cards: Card[]): Threads {
  const byId = new Map(cards.map((c) => [c.id, c]));
  const head = new Map<string, string>();

  const resolve = (card: Card): string => {
    const known = head.get(card.id);
    if (known) return known;
    // Walk up through follow-ups to the first card that is a tile. A malformed
    // parent loop has no such card, so a card caught in one stays a tile
    // rather than vanishing into a thread that has no tile to be drawn in.
    const seen = new Set([card.id]);
    let at = card;
    while (isTurn(at, byId)) {
      const parent = byId.get(at.parent_card_id as string) as Card;
      if (seen.has(parent.id)) {
        head.set(card.id, card.id);
        return card.id;
      }
      seen.add(parent.id);
      at = parent;
    }
    head.set(card.id, at.id);
    return at.id;
  };

  const tiles: Card[] = [];
  const turns = new Map<string, Card[]>();
  for (const c of cards) {
    const h = resolve(c);
    if (h === c.id) {
      tiles.push(c);
      continue;
    }
    const list = turns.get(h);
    if (list) list.push(c);
    else turns.set(h, [c]);
  }

  return { tiles, turns, headOf: (id) => head.get(id) ?? id };
}
