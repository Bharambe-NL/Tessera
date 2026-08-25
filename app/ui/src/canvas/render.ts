/**
 * Card rendering and edge drawing.
 *
 * Ported from the prototype (`canvas-prototype.html:575`-650). The render diff
 * at `canvas-prototype.html:584` is kept and made explicit: a card's markup is
 * rebuilt only when its signature changes, and position is written separately
 * from markup. Under a pan nothing here runs at all, which is what the 200 card
 * gate depends on.
 */

import { CARD_W, DEFAULT_CARD_H } from './layout.js';
import type { Card, Citation } from './types.js';
import { citeMarkers, esc, visualHTML } from './visual.js';

/** Everything that changes a card's markup. Position is deliberately absent. */
function signature(c: Card): string {
  return JSON.stringify([
    c.status,
    c.question,
    c.anchor_text,
    c.kind,
    c.depth,
    c.answer,
    c.confidence,
    c.model_alias,
    c.findings.length,
    c.visual?.id ?? null,
    c.visual?.block_index.map((b) => (b.hidden ? 1 : 0)) ?? null,
    c.citations.map((s) => [s.ordinal, s.verdict, s.stale]),
    c.flags.map((f) => [f.rule_id, f.severity]),
    c.stages.map((s) => [s.label, s.done]),
  ]);
}

function confidenceDot(c: Card): string {
  // Doc 09 section 4: unchecked grey, under 0.5 amber, over 0.5 olive.
  if (c.confidence === null) return `<span class="dot unchecked" title="Unverified"></span>`;
  const tone = c.confidence < 0.5 ? 'low' : 'good';
  return `<span class="dot ${tone}" title="Confidence ${c.confidence.toFixed(2)}"></span>`;
}

function flagChip(c: Card): string {
  if (c.flags.length === 0) return '';
  const worst = c.flags.some((f) => f.severity === 'block')
    ? 'block'
    : c.flags.some((f) => f.severity === 'warn')
      ? 'warn'
      : 'info';
  return `<button class="chip flag ${worst}" data-act="flags" data-no-pan>${c.flags.length}</button>`;
}

function sourcesBlock(c: Card): string {
  if (c.citations.length === 0) return '';
  const rows = c.citations
    .map(
      (s: Citation) =>
        `<li id="src-${esc(c.id)}-${s.ordinal}" class="verdict-${s.verdict}${s.stale ? ' stale' : ''}">` +
        `<span class="ord">${s.ordinal}</span>` +
        `<a href="${esc(s.locator)}" target="_blank" rel="noopener noreferrer">${esc(s.source_title)}</a>` +
        `<span class="cls">${esc(s.source_class)}</span>` +
        (s.stale ? `<span class="stale-tag">stale</span>` : '') +
        `</li>`,
    )
    .join('');
  return `<details class="sources"><summary>Sources (${c.citations.length})</summary><ol>${rows}</ol></details>`;
}

function bodyFor(c: Card): string {
  let body = `<div class="msg">${esc(c.question)}</div>`;

  if (c.status === 'queued' || c.status === 'running') {
    if (c.stages.length === 0) {
      body += `<div class="status dots" role="status" aria-label="Working"></div>`;
    } else {
      // Doc 09 section 4: stages derive from events and tick off in order.
      body += `<div class="stages" role="status">${c.stages
        .map((s) => `<div class="${s.done ? 'done' : 'live'}">${esc(s.label)}${s.done ? '' : '…'}</div>`)
        .join('')}</div>`;
    }
    return body;
  }

  if (c.status === 'failed') {
    body += `<div class="failed">This card did not finish. Rerun it, or open how this was built.</div>`;
    return body;
  }

  if (c.answer) body += `<div class="answer">${citeMarkers(esc(c.answer))}</div>`;
  if (c.findings.length) {
    body += `<div class="findings"><b>Key findings</b>${c.findings
      .map((f) => `<div>${citeMarkers(esc(f.text))}</div>`)
      .join('')}</div>`;
  }
  body += visualHTML(c.visual);
  body += sourcesBlock(c);
  body += `<details class="built"><summary>How this was built</summary><div class="built-body" data-built-for="${esc(
    c.id,
  )}"></div></details>`;
  return body;
}

function cardHTML(c: Card): string {
  const title = c.anchor_text ?? (c.kind === 'follow' ? 'Follow-up' : c.question);
  const depthBadge =
    c.depth !== 'fast' ? `<span class="badge ${c.depth}">${c.depth}</span>` : `<span class="badge fast">fast</span>`;
  const model = c.model_alias ? `<span class="alias" title="Rerun as…">${esc(c.model_alias)}</span>` : '';
  const disabled = c.status === 'done' || c.status === 'flagged' ? '' : 'disabled';

  return (
    `<div class="head">` +
    `<span class="title">${esc(title)}</span>` +
    depthBadge +
    model +
    confidenceDot(c) +
    flagChip(c) +
    `<button class="close" data-act="remove" data-no-pan aria-label="Remove card">✕</button>` +
    `</div>` +
    `<div class="body">${bodyFor(c)}</div>` +
    `<div class="foot">` +
    `<input class="followup" placeholder="Ask a follow-up" ${disabled} data-no-pan aria-label="Ask a follow-up"/>` +
    `<button class="send" ${disabled} data-act="follow" data-no-pan aria-label="Send follow-up">` +
    `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><path d="M12 19V5M5 12l7-7 7 7"/></svg>` +
    `</button>` +
    `</div>`
  );
}

export interface RenderTargets {
  cards: HTMLElement;
  edges: SVGElement;
}

/**
 * Reconcile the card layer against the board. Markup is rebuilt only for cards
 * whose signature changed; every card gets its transform written.
 */
export function renderCards(cards: Card[], targets: RenderTargets): void {
  const seen = new Set<string>();

  for (const c of cards) {
    const id = `card-${c.id}`;
    seen.add(id);
    let el = document.getElementById(id) as HTMLElement | null;

    if (!el) {
      el = document.createElement('article');
      el.id = id;
      el.className = 'card new';
      el.dataset.cardId = c.id;
      targets.cards.appendChild(el);
    }

    const sig = signature(c);
    if (el.dataset.rendered !== sig) {
      el.innerHTML = cardHTML(c);
      el.dataset.rendered = sig;
      el.dataset.status = c.status;
    }

    // Position is a separate write so a move never touches markup.
    el.style.transform = `translate3d(${c.position.x}px, ${c.position.y}px, 0)`;
  }

  for (const el of Array.from(targets.cards.children)) {
    if (!seen.has(el.id)) el.remove();
  }
}

export type HeightLookup = (cardId: string) => number;

/** Measure rendered cards. Called once after a render, never during a pan. */
export function measureHeights(cards: Card[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const c of cards) {
    const el = document.getElementById(`card-${c.id}`);
    m.set(c.id, el ? el.offsetHeight : DEFAULT_CARD_H);
  }
  return m;
}

/**
 * Draw parent to child edges. Follow-ups get an orthogonal drop; branches get a
 * cubic curve to the right. One path element per edge, one write per render.
 */
export function drawEdges(cards: Card[], edges: SVGElement, heightOf: HeightLookup): void {
  const byId = new Map(cards.map((c) => [c.id, c]));
  const follow: string[] = [];
  const branch: string[] = [];

  for (const c of cards) {
    if (c.parent_card_id === null) continue;
    const p = byId.get(c.parent_card_id);
    if (!p) continue;
    const ph = heightOf(p.id);
    const ch = heightOf(c.id);

    if (c.kind === 'branch') {
      const x1 = p.position.x + CARD_W;
      const y1 = p.position.y + Math.min(ph * 0.45, 160);
      const x2 = c.position.x;
      const y2 = c.position.y + Math.min(ch, 120) / 2;
      branch.push(`M${x1},${y1} C${x1 + 60},${y1} ${x2 - 60},${y2} ${x2},${y2}`);
    } else {
      const x1 = p.position.x + CARD_W / 2;
      const y1 = p.position.y + ph;
      const x2 = c.position.x + CARD_W / 2;
      const y2 = c.position.y;
      const my = (y1 + y2) / 2;
      follow.push(`M${x1},${y1} V${my} H${x2} V${y2}`);
    }
  }

  edges.innerHTML =
    `<path class="edge follow" d="${follow.join(' ')}"/>` +
    `<path class="edge branch" d="${branch.join(' ')}"/>`;
}
