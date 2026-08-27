/**
 * The board as a document.
 *
 * Doc 11 section 10: "canvas has a list view alternative (the board's cards as a
 * document) for screen readers, reachable from the title menu."
 *
 * A canvas is a spatial arrangement, and a screen reader has no way to convey
 * the arrangement. This renders the same cards in reading order, parents before
 * their children, with the anchor a branch came from stated rather than drawn as
 * an edge. It is plain HTML with real headings, so it is navigable by heading
 * and by landmark the way a page is.
 *
 * It renders from the same `Card[]` the canvas does, so the two cannot disagree
 * about what the board says.
 */

import { COPY } from '../strings.js';
import type { Board, Card } from './types.js';
import { citeMarkers, esc } from './visual.js';

/** Parents before children, in the order the board holds them. */
function inReadingOrder(cards: Card[]): Card[] {
  const children = new Map<string | null, Card[]>();
  for (const card of cards) {
    const group = children.get(card.parent_card_id) ?? [];
    group.push(card);
    children.set(card.parent_card_id, group);
  }

  const out: Card[] = [];
  const seen = new Set<string>();
  const walk = (parent: string | null, depth: number) => {
    for (const card of children.get(parent) ?? []) {
      if (seen.has(card.id)) continue;
      seen.add(card.id);
      out.push(card);
      // Deep enough for a chain doc 04 caps at three, with room to spare.
      if (depth < 12) walk(card.id, depth + 1);
    }
  };
  walk(null, 0);

  // A card whose parent is not on this board still belongs in the document.
  for (const card of cards) {
    if (!seen.has(card.id)) out.push(card);
  }
  return out;
}

function relation(card: Card): string {
  if (card.kind === 'root') return '';
  if (card.anchor_text) return `${COPY.readingBranchFrom} ${esc(card.anchor_text)}`;
  if (card.anchor_block_ref) return `${COPY.readingBranchFromBlock} ${esc(card.anchor_block_ref)}`;
  return COPY.readingFollowUp;
}

function sources(card: Card): string {
  if (card.citations.length === 0) return '';
  const items = card.citations
    .map(
      (c) =>
        `<li>${c.ordinal}. ${esc(c.source_title)}, ${esc(c.source_class)}` +
        (c.stale ? `, ${COPY.staleTag}` : '') +
        `, ${esc(c.verdict)}</li>`,
    )
    .join('');
  return `<h4>${COPY.sources}</h4><ol>${items}</ol>`;
}

function flags(card: Card): string {
  if (card.flags.length === 0) return '';
  const items = card.flags
    .map((f) => `<li>${esc(f.severity)}, ${esc(f.rule_id)}: ${esc(f.reason)}</li>`)
    .join('');
  return `<h4>${COPY.readingFlags}</h4><ul>${items}</ul>`;
}

function section(card: Card, index: number): string {
  const rel = relation(card);
  const confidence =
    card.confidence === null
      ? COPY.unverified
      : `${COPY.confidence} ${card.confidence.toFixed(2)}`;

  let body = '';
  if (card.status === 'failed') body = `<p>${COPY.cardFailed}</p>`;
  else if (card.answer) body = `<p>${citeMarkers(esc(card.answer))}</p>`;
  else body = `<p>${COPY.readingNoAnswer}</p>`;

  const findings = card.findings.length
    ? `<h4>${COPY.keyFindings}</h4><ul>${card.findings
        .map((f) => `<li>${citeMarkers(esc(f.text))}</li>`)
        .join('')}</ul>`
    : '';

  // The visual is described rather than drawn: its type, its title and the
  // labels of its blocks, which is what a reader who cannot see it needs.
  const visual = card.visual
    ? `<h4>${esc(card.visual.title)}</h4><p>${COPY.readingVisualType} ${esc(card.visual.type)}.</p>` +
      `<ul>${card.visual.block_index
        .filter((b) => !b.hidden)
        .map((b) => `<li>${esc(b.label)}</li>`)
        .join('')}</ul>`
    : '';

  return (
    `<section aria-labelledby="reading-h-${esc(card.id)}">` +
    `<h3 id="reading-h-${esc(card.id)}">${index + 1}. ${esc(card.question)}</h3>` +
    `<p class="meta">${esc(card.depth)}, ${esc(confidence)}${rel ? `, ${rel}` : ''}</p>` +
    body +
    findings +
    visual +
    sources(card) +
    flags(card) +
    `</section>`
  );
}

export function readingHTML(board: Board): string {
  const cards = inReadingOrder(board.cards);
  if (cards.length === 0) return `<p>${COPY.readingEmpty}</p>`;
  return (
    `<h2>${esc(board.title)}</h2>` +
    `<p class="meta">${cards.length} ${COPY.homeCards}</p>` +
    cards.map(section).join('')
  );
}
