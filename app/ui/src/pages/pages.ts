/**
 * Pages. Doc 16 section 3.7's rail item and phase 12c.
 *
 * "Explorer, editor with write and preview, backlinks panel, unresolved link
 * creation." Two states in one view: a list of what the vault holds, and one
 * page open for reading and writing.
 *
 * The preview is markdown rendered to the same shapes an answer uses, with one
 * addition: a wikilink is a control rather than text. A resolved one opens the
 * page it names; an unresolved one offers to write it, which is doc 16 section
 * 3.1's "creates the page on click" and the reason an unresolved link is kept
 * rather than dropped.
 */

import { esc } from '../canvas/visual.js';
import type { PageDetail, PageRow } from '../rpc.js';
import { COPY } from '../strings.js';
import { ago, emptyState } from './shared.js';

export interface PagesState {
  /** The page being read, or `null` for the explorer. */
  open: PageDetail | null;
  /** Whether the open page is being written rather than read. */
  editing: boolean;
}

export function pagesToolsHTML(state: PagesState): string {
  if (!state.open) {
    return `<div class="seg"><button data-page-act="new">${COPY.pagesNew}</button></div>`;
  }
  return (
    `<div class="seg">` +
    `<button data-page-act="close">${COPY.pagesBack}</button>` +
    `<button data-page-act="${state.editing ? 'preview' : 'edit'}"${state.editing ? '' : ' class="on"'}>` +
    `${state.editing ? COPY.pagesPreview : COPY.pagesEdit}</button>` +
    `</div>`
  );
}

function row(p: PageRow): string {
  // Doc 16 section 3.2: a page saved from a card carries the card's citations,
  // and that is worth seeing without opening it, because it is the difference
  // between a page that can support a claim and one that is only context.
  // A count of zero is not worth a chip that reads like one: a card that cited
  // nothing makes a page that carries nothing, and saying "0 carried citations"
  // states the same thing twice while looking like a measurement.
  const carried = !p.from_card
    ? ''
    : p.citations_carried > 0
      ? `<span class="chip">${p.citations_carried} ${COPY.pagesCarried}</span>`
      : `<span class="chip">${COPY.pagesFromCard}</span>`;
  return (
    `<li class="lib-row" data-page="${esc(p.id)}">` +
    `<div class="what"><div class="line">` +
    `<span class="title">${esc(p.title)}</span>${carried}</div>` +
    `<p class="meta">${esc(p.file_path)}, ${COPY.pagesEdited} ${esc(ago(p.updated_at))}</p>` +
    `</div>` +
    `<div class="verbs">` +
    `<button data-page-act="open">${COPY.pagesOpen}</button>` +
    `<button class="danger" data-page-act="remove">${COPY.pagesRemove}</button>` +
    `</div></li>`
  );
}

/**
 * Markdown as much as a vault needs: headings, list items, paragraphs, and
 * wikilinks as controls.
 *
 * Not a markdown library. What a page written here contains is what the app
 * writes into it plus what a person types, and the shapes below are those. A
 * page that uses more renders as the text it is, which is the honest failure
 * for a preview.
 */
function preview(body: string, links: PageDetail['links']): string {
  const unresolved = new Set(
    links.filter((l) => l.target_kind === 'unresolved').map((l) => l.target_title),
  );
  const out: string[] = [];
  let list: string[] = [];

  const flush = () => {
    if (list.length) out.push(`<ul>${list.join('')}</ul>`);
    list = [];
  };

  for (const line of body.split('\n')) {
    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      flush();
      const level = Math.min(heading[1].length + 1, 5);
      out.push(`<h${level}>${inline(heading[2], unresolved)}</h${level}>`);
      continue;
    }
    const item = /^[-*]\s+(.*)$/.exec(line);
    if (item) {
      list.push(`<li>${inline(item[1], unresolved)}</li>`);
      continue;
    }
    if (!line.trim()) {
      flush();
      continue;
    }
    flush();
    out.push(`<p>${inline(line, unresolved)}</p>`);
  }
  flush();
  return out.join('') || emptyState(COPY.pagesEmptyBody);
}

/** Wikilinks to controls, everything else escaped. */
function inline(text: string, unresolved: Set<string>): string {
  let out = '';
  let rest = text;
  for (;;) {
    const at = rest.indexOf('[[');
    const end = at < 0 ? -1 : rest.indexOf(']]', at);
    if (at < 0 || end < 0) {
      out += esc(rest);
      return out;
    }
    out += esc(rest.slice(0, at));
    const inner = rest.slice(at + 2, end);
    const bar = inner.indexOf('|');
    const target = (bar < 0 ? inner : inner.slice(0, bar)).trim();
    const shown = (bar < 0 ? inner : inner.slice(bar + 1)).trim() || target;
    const missing = unresolved.has(target);
    out +=
      `<button class="wikilink${missing ? ' unresolved' : ''}" ` +
      `data-page-act="${missing ? 'create-link' : 'follow-link'}" ` +
      `data-title="${esc(target)}">${esc(shown)}</button>`;
    rest = rest.slice(end + 2);
  }
}

function backlinksPanel(page: PageDetail): string {
  if (page.backlinks.length === 0) return `<p class="page-note">${COPY.pagesNoBacklinks}</p>`;
  return (
    `<h3>${COPY.pagesBacklinks}</h3><ul class="lib-list">` +
    page.backlinks
      .map(
        (b) =>
          `<li class="lib-row" data-page="${esc(b.page_id)}"><div class="what"><div class="line">` +
          `<span class="title">${esc(b.title)}</span></div>` +
          `<p class="meta">${esc(b.display_text)}</p></div>` +
          `<div class="verbs"><button data-page-act="open">${COPY.pagesOpen}</button></div></li>`,
      )
      .join('') +
    `</ul>`
  );
}

export function pagesHTML(rows: PageRow[], state: PagesState): string {
  if (!state.open) {
    if (rows.length === 0) return emptyState(COPY.pagesEmpty);
    return `<ul class="lib-list">${rows.map(row).join('')}</ul>`;
  }

  const page = state.open;
  if (state.editing) {
    return (
      `<form id="page-edit" class="page-edit">` +
      `<input id="page-name" value="${esc(page.title)}" aria-label="${COPY.pagesTitle}" ` +
      `autocomplete="off" />` +
      `<textarea id="page-text" rows="18" aria-label="${COPY.pagesBody}" ` +
      `spellcheck="false">${esc(page.body)}</textarea>` +
      `<div class="setup-acts"><button type="submit" class="primary">${COPY.pagesSave}</button>` +
      `<span class="note page-file">${COPY.pagesFileNote} ${esc(page.file_path)}</span></div>` +
      `</form>`
    );
  }

  return (
    `<article class="page-read">${preview(page.body, page.links)}</article>` +
    `<p class="page-note page-file">${COPY.pagesFileNote} ${esc(page.file_path)}</p>` +
    backlinksPanel(page)
  );
}
