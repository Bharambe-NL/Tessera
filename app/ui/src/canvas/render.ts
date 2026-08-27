/**
 * Card rendering and edge drawing.
 *
 * Ported from the prototype (`canvas-prototype.html:575`-650). The render diff
 * at `canvas-prototype.html:584` is kept and made explicit: a card's markup is
 * rebuilt only when its signature changes, and position is written separately
 * from markup. Under a pan nothing here runs at all, which is what the 200 card
 * gate depends on.
 */

import { COPY } from '../strings.js';
import { CARD_W, DEFAULT_CARD_H } from './layout.js';
import type { Card, Citation, FlagSummary } from './types.js';
import { citeMarkers, esc, visualHTML } from './visual.js';

/**
 * Cards whose flag list is open.
 *
 * View state, so it lives with the view rather than on the read model: the core
 * has no opinion about which disclosures a reader has expanded, and putting it
 * on `Card` would make it something a reload could silently revert.
 */
const flagsOpen = new Set<string>();

export function toggleFlags(cardId: string): boolean {
  if (flagsOpen.has(cardId)) {
    flagsOpen.delete(cardId);
    return false;
  }
  flagsOpen.add(cardId);
  return true;
}

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
    // Without this the chip would appear only after some other change forced
    // the card's markup to be rebuilt.
    c.page_id ?? null,
    c.stages.map((s) => [s.label, s.done]),
    flagsOpen.has(c.id),
  ]);
}

/**
 * Doc 16 section 4: the card header shows a page chip once it has been saved.
 *
 * A chip rather than a link, because the Pages view is 12c and a chip that led
 * nowhere would be a promise the shell cannot keep yet.
 */
function pageChip(c: Card): string {
  if (!c.page_id) return '';
  return `<span class="chip page" data-page="${esc(c.page_id)}">${COPY.savedAsPage}</span>`;
}

function confidenceDot(c: Card): string {
  // Doc 09 section 4: unchecked grey, under 0.5 amber, over 0.5 olive.
  if (c.confidence === null) return `<span class="dot unchecked" title="${COPY.unverified}"></span>`;
  const tone = c.confidence < 0.5 ? 'low' : 'good';
  return `<span class="dot ${tone}" title="${COPY.confidence} ${c.confidence.toFixed(2)}"></span>`;
}

function worstSeverity(flags: FlagSummary[]): string {
  if (flags.some((f) => f.severity === 'block')) return 'block';
  return flags.some((f) => f.severity === 'warn') ? 'warn' : 'info';
}

function flagChip(c: Card): string {
  if (c.flags.length === 0) return '';
  const open = flagsOpen.has(c.id);
  const label = open ? COPY.closeFlags : COPY.openFlags;
  return (
    `<button class="chip flag ${worstSeverity(c.flags)}" data-act="flags" data-no-pan ` +
    `aria-expanded="${open}" aria-label="${label}">${c.flags.length}</button>`
  );
}

/**
 * The card's own flags, shown where the chip was clicked.
 *
 * Doc 09 section 5 reads Open on a flag as "go to the card, target highlighted",
 * and the reader is already on the card. Accept and Dismiss arrive with the
 * Flags queue, which is where a decision is recorded.
 */
function flagList(c: Card): string {
  if (!flagsOpen.has(c.id) || c.flags.length === 0) return '';
  const rows = c.flags
    .map(
      (f) =>
        `<li class="sev-${esc(f.severity)}">` +
        `<span class="rule">${esc(f.rule_id)}</span>` +
        `<span class="reason">${esc(f.reason)}</span>` +
        `</li>`,
    )
    .join('');
  return `<ul class="flag-list" data-flags-for="${esc(c.id)}">${rows}</ul>`;
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
        (s.stale ? `<span class="stale-tag">${COPY.staleTag}</span>` : '') +
        `</li>`,
    )
    .join('');
  return (
    `<details class="sources"><summary>${COPY.sources} (${c.citations.length})</summary>` +
    `<ol>${rows}</ol></details>`
  );
}

function bodyFor(c: Card): string {
  // A read card's question is the product's, not the reader's, so it does not
  // get the bubble that shows what someone asked.
  let body = c.kind === 'read' ? '' : `<div class="msg">${esc(c.question)}</div>`;
  body += flagList(c);

  if (c.status === 'queued' || c.status === 'running') {
    if (c.stages.length === 0) {
      body += `<div class="status dots" role="status" aria-label="${COPY.working}"></div>`;
    } else {
      // Doc 09 section 4: stages derive from events and tick off in order.
      body += `<div class="stages" role="status">${c.stages
        .map((s) => `<div class="${s.done ? 'done' : 'live'}">${esc(s.label)}${s.done ? '' : '…'}</div>`)
        .join('')}</div>`;
    }
    return body;
  }

  if (c.status === 'failed') {
    body += `<div class="failed">${COPY.cardFailed}</div>`;
    return body;
  }

  if (c.answer) body += `<div class="answer">${citeMarkers(esc(c.answer))}</div>`;
  if (c.findings.length) {
    body += `<div class="findings"><b>${COPY.keyFindings}</b>${c.findings
      .map((f) => `<div>${citeMarkers(esc(f.text))}</div>`)
      .join('')}</div>`;
  }
  body += visualHTML(c.visual);
  body += sourcesBlock(c);
  body += `<details class="built"><summary>${COPY.howBuilt}</summary><div class="built-body" data-built-for="${esc(
    c.id,
  )}"></div></details>`;
  return body;
}

function cardHTML(c: Card): string {
  // Doc 07 section A11: "Reader cards show 'Read from image' in the header".
  // A read card's question is one nobody typed, so showing it as a title would
  // put words in the reader's mouth.
  const title =
    c.kind === 'read'
      ? COPY.readFromImage
      : (c.anchor_text ?? (c.kind === 'root' ? c.question : COPY.followTitle));
  const depthBadge =
    c.depth !== 'fast' ? `<span class="badge ${c.depth}">${c.depth}</span>` : `<span class="badge fast">fast</span>`;
  const model = c.model_alias ? `<span class="alias" title="${COPY.rerunAs}">${esc(c.model_alias)}</span>` : '';
  // Doc 09 section 5's verbs act on a card that has an answer to act on. A
  // running card has nothing to follow up or check again yet.
  const settled = c.status === 'done' || c.status === 'flagged';
  const disabled = settled ? '' : 'disabled';

  return (
    `<div class="head">` +
    `<span class="title">${esc(title)}</span>` +
    depthBadge +
    model +
    confidenceDot(c) +
    flagChip(c) +
    pageChip(c) +
    // Doc 16 section 3.2's ninth verb, offered only where there is something to
    // keep. A blocked card is refused by the core; hiding the verb on one that
    // has no answer yet keeps the refusal for the case a person cannot see.
    (c.page_id
      ? ''
      : `<button class="save" data-act="save" ${disabled} data-no-pan ` +
        `aria-label="${COPY.saveAsPage}" title="${COPY.saveAsPage}">` +
        `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2zM7 3v6h8M7 21v-8h10v8"/></svg>` +
        `</button>`) +
    `<button class="rerun" data-act="rerun" ${disabled} data-no-pan aria-label="${COPY.rerunCard}">` +
    `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6"/></svg>` +
    `</button>` +
    `</div>` +
    // `data-no-pan` because a drag inside the body is a person selecting a span
    // to branch from, and the viewport's drag handler would pan the board out
    // from under them instead. Doc 09 section 3's highlight popover depends on
    // the selection surviving the pointer.
    `<div class="body" data-no-pan>${bodyFor(c)}</div>` +
    `<div class="foot">` +
    `<input class="followup" placeholder="${COPY.askFollowUp}" ${disabled} data-no-pan aria-label="${COPY.askFollowUp}"/>` +
    `<button class="send" ${disabled} data-act="follow" data-no-pan aria-label="${COPY.sendFollowUp}">` +
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
