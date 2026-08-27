/**
 * Deterministic board fixtures for the M0 performance gate.
 *
 * The gate measures pan at 200 cards, so the cards have to weigh what real ones
 * weigh: prose, findings, a visual with a populated block index, citations, and
 * a sources disclosure. A board of 200 empty divs would pass a gate that a real
 * board fails.
 *
 * Seeded so a regression is comparable across runs.
 */

import type { Board, Card, CardKind, Depth, Visual, VisualType } from '../canvas/types.js';

/** mulberry32. Small, fast, good enough for fixture shape. */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const WORDS =
  'capital buffer exposure threshold article obligation counterparty settlement mandate disclosure liquidity ratio supervisory review consolidated treatment trading book issuer register scope framework'.split(
    ' ',
  );

function sentence(r: () => number, n: number): string {
  const w: string[] = [];
  for (let i = 0; i < n; i++) w.push(WORDS[Math.floor(r() * WORDS.length)]);
  const s = w.join(' ');
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function prose(r: () => number, sentences: number, cites: number): string {
  const out: string[] = [];
  for (let i = 0; i < sentences; i++) {
    const marker = i < cites ? ` [${i + 1}]` : '';
    out.push(`${sentence(r, 8 + Math.floor(r() * 10))}${marker}.`);
  }
  return out.join(' ');
}

const TYPES: VisualType[] = ['tree', 'table', 'list', 'steps', 'figure'];

function makeVisual(r: () => number, id: string, type: VisualType): Visual {
  switch (type) {
    case 'tree': {
      const children = Array.from({ length: 3 + Math.floor(r() * 3) }, (_, i) => ({
        label: sentence(r, 2),
        note: sentence(r, 6),
        citation_ordinals: [1 + (i % 3)],
      }));
      return {
        id,
        type,
        title: sentence(r, 3),
        payload: { root: { label: sentence(r, 2), children } },
        block_index: [
          { ref: '/root', label: 'root', citation_ordinals: [1] },
          ...children.map((c, i) => ({
            ref: `/root/children/${i}`,
            label: c.label,
            citation_ordinals: c.citation_ordinals,
          })),
        ],
      };
    }
    case 'table': {
      const rows = Array.from({ length: 4 + Math.floor(r() * 3) }, () => [sentence(r, 3), sentence(r, 4)]);
      return {
        id,
        type,
        title: sentence(r, 3),
        payload: {
          columns: ['Under the old rule', 'Under the new rule'],
          rows,
          bottom_line: { head: 'Bottom line', text: sentence(r, 10), citation_ordinals: [1] },
        },
        block_index: [
          { ref: '/columns/0', label: 'col 0', citation_ordinals: [], no_claim: true },
          { ref: '/columns/1', label: 'col 1', citation_ordinals: [], no_claim: true },
          ...rows.flatMap((row, ri) =>
            row.map((cell, ci) => ({
              ref: `/rows/${ri}/${ci}`,
              label: cell,
              citation_ordinals: [1 + ((ri + ci) % 3)],
            })),
          ),
        ],
      };
    }
    case 'list': {
      const groups = Array.from({ length: 2 }, () => ({
        heading: sentence(r, 2),
        items: Array.from({ length: 2 + Math.floor(r() * 3) }, () => ({
          name: sentence(r, 2),
          detail: sentence(r, 6),
        })),
      }));
      return {
        id,
        type,
        title: sentence(r, 3),
        payload: { groups, bottom_line: { head: 'All have', text: sentence(r, 8) } },
        block_index: groups.flatMap((g, gi) => [
          { ref: `/groups/${gi}/heading`, label: g.heading, citation_ordinals: [], no_claim: true },
          ...g.items.map((it, ii) => ({
            ref: `/groups/${gi}/items/${ii}`,
            label: it.name,
            citation_ordinals: [1 + (ii % 3)],
          })),
        ]),
      };
    }
    case 'steps': {
      const steps = Array.from({ length: 3 + Math.floor(r() * 3) }, () => ({
        label: sentence(r, 2),
        note: sentence(r, 7),
      }));
      return {
        id,
        type,
        title: sentence(r, 3),
        payload: { steps },
        block_index: steps.map((s, i) => ({
          ref: `/steps/${i}`,
          label: s.label,
          citation_ordinals: [1 + (i % 3)],
        })),
      };
    }
    default: {
      return {
        id,
        type: 'figure',
        title: sentence(r, 3),
        payload: {
          svg: '<svg viewBox="0 0 200 200" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M40 70 Q100 30 160 70"/><path d="M100 52 V110 M100 78 L84 110 M100 78 L116 110"/><path d="M45 110 H155 M45 110 V180 M155 110 V180"/></svg>',
          caption: sentence(r, 5),
        },
        block_index: [{ ref: '/svg', label: 'figure', citation_ordinals: [], no_claim: true }],
      };
    }
  }
}

function makeCard(r: () => number, i: number, parent: string | null, kind: CardKind): Card {
  const depth: Depth = i % 5 === 0 ? 'research' : i % 2 === 0 ? 'deep' : 'fast';
  const citeCount = depth === 'fast' ? 0 : 3;
  const id = `c${String(i).padStart(4, '0')}`;

  return {
    id,
    parent_card_id: parent,
    kind,
    anchor_text: kind === 'branch' ? sentence(r, 2) : null,
    anchor_block_ref: kind === 'branch' ? '/root/children/1' : null,
    question: sentence(r, 6) + '?',
    depth,
    audience_id: null,
    answer: prose(r, 4, citeCount),
    findings:
      depth === 'fast'
        ? []
        : Array.from({ length: 3 }, (_, k) => ({
            text: `${sentence(r, 12)} [${k + 1}]`,
            citation_ordinals: [k + 1],
          })),
    visual: makeVisual(r, `v${id}`, TYPES[i % TYPES.length]),
    citations: Array.from({ length: citeCount }, (_, k) => ({
      ordinal: k + 1,
      source_title: sentence(r, 5),
      source_class: k === 0 ? 'regulatory' : 'web',
      locator: `https://example.invalid/${id}/${k}`,
      verdict: k === 2 ? 'weak' : 'supported',
      stale: false,
    })),
    flags: i % 11 === 0 ? [{ id: `f${id}`, rule_id: 'stale_source', severity: 'warn', reason: sentence(r, 8) }] : [],
    status: i % 11 === 0 ? 'flagged' : 'done',
    confidence: depth === 'fast' ? null : 0.4 + r() * 0.55,
    model_alias: depth === 'fast' ? 'medium' : 'frontier',
    stages: [],
    position: { x: 0, y: 0, dx: 0, dy: 0, pinned: false },
  };
}

/**
 * A board of `count` cards in a realistic tree: roots with follow-ups below and
 * branches to the right, matching what the layout algorithm actually walks.
 */
export function makeBoard(count: number, seed = 42): Board {
  const r = rng(seed);
  const cards: Card[] = [];
  const roots: string[] = [];

  let i = 0;
  while (cards.length < count) {
    const root = makeCard(r, i++, null, 'root');
    cards.push(root);
    roots.push(root.id);

    // Two to four follow-ups, each with zero to two branches.
    const follows = 2 + Math.floor(r() * 3);
    for (let f = 0; f < follows && cards.length < count; f++) {
      const fc = makeCard(r, i++, root.id, 'follow');
      cards.push(fc);
      const branches = Math.floor(r() * 3);
      for (let b = 0; b < branches && cards.length < count; b++) {
        cards.push(makeCard(r, i++, fc.id, 'branch'));
      }
    }
  }

  return {
    id: 'perf-board',
    title: `Performance fixture, ${cards.length} cards`,
    named_by_user: true,
    doctrine_pack: { code: 'general', version: '1.0.0' },
    default_depth: 'deep',
    mode: 'explore',
    parent_board_id: null,
    seed_label: null,
    notes: [],
    viewport: { x: 0, y: 0, k: 1 },
    cards: cards.slice(0, count),
  };
}
