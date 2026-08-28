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
import { button } from '../ui/button.js';
import { chip } from '../ui/chip.js';
import { segmented } from '../ui/segmented.js';
import { ago, emptyState } from './shared.js';

export type LibraryTab = 'sources' | 'concepts';

export function libraryToolsHTML(tab: LibraryTab): string {
  return segmented(
    [
      { label: COPY.librarySources, on: tab === 'sources', data: { 'library-tab': 'sources' } },
      { label: COPY.libraryConcepts, on: tab === 'concepts', data: { 'library-tab': 'concepts' } },
    ],
    { ariaLabel: COPY.libraryTabsLabel },
  );
}

function sourceRow(s: SourceRow): string {
  const stale = s.stale
    ? chip(COPY.staleTag, { classes: 'stale', title: s.stale_reason ?? '' })
    : '';
  const verified = s.last_verified_at ? ago(s.last_verified_at) : COPY.libraryNeverVerified;
  return (
    `<li class="lib-row" data-source="${esc(s.id)}">` +
    `<div class="what">` +
    `<div class="line"><span class="title">${esc(s.title)}</span>${stale}` +
    chip(s.class) +
    chip(String(s.trust_rank), { title: COPY.libraryTrustRank }) +
    `</div>` +
    `<p class="meta">${esc(s.issuer ?? COPY.libraryNoIssuer)}, ` +
    `${s.cards} ${COPY.libraryCitedOn}, ${COPY.libraryVerified} ${esc(verified)}</p>` +
    `</div>` +
    `<div class="verbs">` +
    button(COPY.libraryOpen, { data: { 'source-act': 'open' } }) +
    button(COPY.libraryAsk, { data: { 'source-act': 'ask' } }) +
    // Doc 09 section 5: Remove on a source only if it is uncited.
    (s.cards === 0
      ? button(COPY.libraryRemove, { variant: 'danger', data: { 'source-act': 'remove' } })
      : '') +
    `</div>` +
    `</li>`
  );
}

export function sourcesHTML(sources: SourceRow[]): string {
  if (sources.length === 0) return emptyState(COPY.libraryNoSources);
  return `<ul class="lib-list">${sources.map(sourceRow).join('')}</ul>`;
}

function conceptRow(c: ConceptRow): string {
  const status = chip(c.status, { classes: `status-${c.status}` });
  const definition = c.definition
    ? `<p class="reason">${esc(c.definition)}</p>`
    : `<p class="reason muted">${COPY.libraryNoDefinition}</p>`;
  const decide =
    c.status === 'proposed'
      ? button(COPY.libraryAccept, { data: { 'concept-act': 'accept' } }) +
        button(COPY.libraryDismiss, { data: { 'concept-act': 'dismiss' } })
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
    button(COPY.libraryAsk, { data: { 'concept-act': 'ask' } }) +
    `</div>` +
    `</li>`
  );
}

export function conceptsHTML(concepts: ConceptRow[]): string {
  if (concepts.length === 0) return emptyState(COPY.libraryNoConcepts);
  return `<ul class="lib-list">${concepts.map(conceptRow).join('')}</ul>`;
}
