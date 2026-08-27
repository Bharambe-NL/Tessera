/**
 * What every page needs and none of them should write twice.
 *
 * Doc 11 section 4: hand built components in the prototype's style, shared
 * primitives only. The queue row, the empty state and the relative date are the
 * three that appear on more than one page.
 */

import { esc } from '../canvas/visual.js';
import { COPY } from '../strings.js';

/** A page that has nothing to show says what would put something there. */
export function emptyState(message: string): string {
  return `<p class="page-empty">${esc(message)}</p>`;
}

/**
 * How long ago, in the coarsest unit that still says something.
 *
 * Doc 09 section 6 puts an age on every queue row. A timestamp to the second
 * is precision nobody reads; "3 days" is the decision the reader is making.
 */
export function ago(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return COPY.agoUnknown;
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 60) return COPY.agoNow;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}${COPY.agoMinutes}`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}${COPY.agoHours}`;
  const days = Math.round(hours / 24);
  if (days < 31) return `${days}${COPY.agoDays}`;
  return `${Math.round(days / 30)}${COPY.agoMonths}`;
}

/** The severity chip doc 09 section 6 puts at the head of every flag row. */
export function severityChip(severity: string): string {
  return `<span class="chip sev ${esc(severity)}">${esc(severity)}</span>`;
}

/**
 * A short readable excerpt of whatever a rule wrote as evidence.
 *
 * Doc 09 section 6 wants "evidence preview (passage excerpt or stale date)",
 * and the rules write different shapes into that column. Rather than teach this
 * every rule's shape, it reads the fields that carry prose and falls back to
 * saying there is none, which is honest about a rule that wrote nothing.
 */
export function evidenceLine(evidence: unknown): string {
  if (typeof evidence === 'string') return evidence;
  if (!evidence || typeof evidence !== 'object') return '';
  const e = evidence as Record<string, unknown>;
  for (const key of ['passage_text', 'excerpt', 'text', 'detail', 'locator', 'stale_reason']) {
    const value = e[key];
    if (typeof value === 'string' && value.trim()) {
      return value.length > 160 ? `${value.slice(0, 160)}…` : value;
    }
  }
  return '';
}
