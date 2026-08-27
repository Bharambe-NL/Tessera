/**
 * Library. Doc 09 section 9: two tabs.
 *
 * Sources: "title, issuer, class, trust rank, cited on n cards, last verified,
 * stale state". Concepts: "term, status (proposed or confirmed), definition,
 * audience definitions, linked cards".
 *
 * Both row action sets are doc 09 section 5's verbs narrowed by what the row
 * allows: Remove on a source only when it is uncited, on a concept only when it
 * is unlinked, so the button is absent rather than present and refused.
 */

import { esc } from '../canvas/visual.js';
import type { ConceptRow, SourceRow } from '../rpc.js';
import { COPY } from '../strings.js';
import { ago, emptyState } from './shared.js';

export type LibraryTab = 'sources' | 'concepts';

export function libraryToolsHTML(tab: LibraryTab): string {
  const on = (t: LibraryTab) => (t === tab ? ' class="on"' : '');
  return (
    `<div class="seg" role="group" aria-label="${COPY.libraryTabsLabel}">` +
    `<button data-library-tab="sources"${on('sources')}>${COPY.librarySources}</button>` +
    `<button data-library-tab="concepts"${on('concepts')}>${COPY.libraryConcepts}</button>` +
    `</div>`
  );
}

function sourceRow(s: SourceRow): string {
  const stale = s.stale
    ? `<span class="chip stale" title="${esc(s.stale_reason ?? '')}">${COPY.staleTag}</span>`
    : '';
  const verified = s.last_verified_at ? ago(s.last_verified_at) : COPY.libraryNeverVerified;
  return (
    `<li class="lib-row" data-source="${esc(s.id)}">` +
    `<div class="what">` +
    `<div class="line"><span class="title">${esc(s.title)}</span>${stale}` +
    `<span class="chip">${esc(s.class)}</span>` +
    `<span class="chip" title="${COPY.libraryTrustRank}">${s.trust_rank}</span></div>` +
    `<p class="meta">${esc(s.issuer ?? COPY.libraryNoIssuer)}, ` +
    `${s.cards} ${COPY.libraryCitedOn}, ${COPY.libraryVerified} ${esc(verified)}</p>` +
    `</div>` +
    `<div class="verbs">` +
    `<button data-source-act="open">${COPY.libraryOpen}</button>` +
    `<button data-source-act="ask">${COPY.libraryAsk}</button>` +
    // Doc 09 section 5: Remove on a source only if it is uncited.
    (s.cards === 0 ? `<button class="danger" data-source-act="remove">${COPY.libraryRemove}</button>` : '') +
    `</div>` +
    `</li>`
  );
}

export function sourcesHTML(sources: SourceRow[]): string {
  if (sources.length === 0) return emptyState(COPY.libraryNoSources);
  return `<ul class="lib-list">${sources.map(sourceRow).join('')}</ul>`;
}

function conceptRow(c: ConceptRow): string {
  const status = `<span class="chip status-${esc(c.status)}">${esc(c.status)}</span>`;
  const definition = c.definition
    ? `<p class="reason">${esc(c.definition)}</p>`
    : `<p class="reason muted">${COPY.libraryNoDefinition}</p>`;
  const decide =
    c.status === 'proposed'
      ? `<button data-concept-act="accept">${COPY.libraryAccept}</button>` +
        `<button data-concept-act="dismiss">${COPY.libraryDismiss}</button>`
      : '';
  return (
    `<li class="lib-row" data-concept="${esc(c.id)}">` +
    `<div class="what">` +
    `<div class="line"><span class="title">${esc(c.term)}</span>${status}` +
    `<span class="age">${c.links} ${COPY.libraryLinks}</span></div>` +
    definition +
    `</div>` +
    `<div class="verbs">` +
    decide +
    `<button data-concept-act="ask">${COPY.libraryAsk}</button>` +
    `</div>` +
    `</li>`
  );
}

export function conceptsHTML(concepts: ConceptRow[]): string {
  if (concepts.length === 0) return emptyState(COPY.libraryNoConcepts);
  return `<ul class="lib-list">${concepts.map(conceptRow).join('')}</ul>`;
}
