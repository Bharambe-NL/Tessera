/**
 * The Flags queue. Doc 09 section 6, the densest screen in the product.
 *
 * "Rows grouped by board, sorted by severity then age. Each row: severity chip,
 * rule name, reason, card title, age, evidence preview. Row actions: Open,
 * Accept, Dismiss, Rerun. Bulk: select by rule, select by board. Bulk Dismiss
 * requires a second click with the count shown. Bulk Accept needs no
 * confirmation."
 *
 * The core sorts by severity then age; the grouping is done here, because it is
 * presentation and the same rows feed a future ungrouped view.
 */

import { esc } from '../canvas/visual.js';
import type { FlagRow } from '../rpc.js';
import { COPY } from '../strings.js';
import { ago, emptyState, evidenceLine, severityChip } from './shared.js';

function row(flag: FlagRow, selected: boolean): string {
  const evidence = evidenceLine(flag.evidence);
  return (
    // Doc 09 section 14: "flag rows navigable with arrows". The row itself is
    // the stop, so a reader moves between rows and tabs into one when they mean
    // to act on it, rather than tabbing through four verbs per row to reach the
    // next one.
    `<li class="flag-row" tabindex="0" data-flag="${esc(flag.id)}" data-rule="${esc(flag.rule_id)}">` +
    `<input type="checkbox" class="pick" ${selected ? 'checked' : ''} ` +
    `aria-label="${COPY.flagsSelectRow}" />` +
    `<div class="what">` +
    `<div class="line">${severityChip(flag.severity)}` +
    `<span class="rule">${esc(flag.rule_id)}</span>` +
    `<span class="card-title">${esc(flag.card_title)}</span>` +
    `<span class="age">${esc(ago(flag.created_at))}</span></div>` +
    `<p class="reason">${esc(flag.reason)}</p>` +
    (evidence ? `<p class="evidence">${esc(evidence)}</p>` : '') +
    `</div>` +
    `<div class="verbs">` +
    `<button data-flag-act="open">${COPY.flagsOpen}</button>` +
    `<button data-flag-act="accept">${COPY.flagsAccept}</button>` +
    `<button data-flag-act="dismiss">${COPY.flagsDismiss}</button>` +
    `<button data-flag-act="rerun">${COPY.flagsRerun}</button>` +
    `</div>` +
    `</li>`
  );
}

export function flagsHTML(flags: FlagRow[], selected: Set<string>): string {
  if (flags.length === 0) return emptyState(COPY.flagsNone);

  // Grouped by board, in the order the boards first appear, which is severity
  // then age because that is how the core returned them.
  const groups = new Map<string, { title: string; rows: FlagRow[] }>();
  for (const flag of flags) {
    const group = groups.get(flag.board_id) ?? { title: flag.board_title, rows: [] };
    group.rows.push(flag);
    groups.set(flag.board_id, group);
  }

  return [...groups.entries()]
    .map(
      ([boardId, group]) =>
        `<section class="flag-group" data-board="${esc(boardId)}">` +
        `<h2>${esc(group.title)}` +
        `<button class="select-board" data-select-board="${esc(boardId)}">${COPY.flagsSelectBoard}</button>` +
        `</h2>` +
        `<ul>${group.rows.map((f) => row(f, selected.has(f.id))).join('')}</ul>` +
        `</section>`,
    )
    .join('');
}

/**
 * The bulk bar, shown only when something is selected.
 *
 * Doc 09 section 6: "Bulk Dismiss requires a second click with the count shown.
 * Bulk Accept needs no confirmation." Accepting leaves the content hidden and
 * the flag standing, so it costs nothing to undo; dismissing reveals content a
 * rule objected to, which is the decision worth a second look.
 */
export function bulkHTML(selected: number, confirmingDismiss: boolean): string {
  if (selected === 0) return '';
  const dismiss = confirmingDismiss
    ? `<button class="danger" data-bulk="dismiss-confirm">${COPY.flagsDismissConfirm} ${selected}</button>`
    : `<button data-bulk="dismiss">${COPY.flagsDismiss}</button>`;
  return (
    `<div class="bulk" role="group" aria-label="${COPY.flagsBulkLabel}">` +
    `<span class="count">${selected} ${COPY.flagsSelected}</span>` +
    `<button data-bulk="accept">${COPY.flagsAccept}</button>` +
    dismiss +
    `<button data-bulk="clear">${COPY.flagsClear}</button>` +
    `</div>`
  );
}
