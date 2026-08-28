/**
 * The segmented control: one choice showing, the chosen one in accent.
 *
 * Doc 11 section 3 gives "selected" to the accent, and components.css holds
 * the rule; what a call site provides is the items, which one is on, and the
 * data attribute its delegated click handler reads back.
 */

import { esc } from '../canvas/visual.js';

export interface SegmentedItem {
  label: string;
  on?: boolean;
  /** data-* attributes, keys given without the data- prefix. */
  data?: Record<string, string>;
}

export function segmented(items: SegmentedItem[], opts: { ariaLabel?: string } = {}): string {
  const buttons = items
    .map((item) => {
      const data = Object.entries(item.data ?? {})
        .map(([key, value]) => ` data-${key}="${esc(value)}"`)
        .join('');
      return `<button type="button"${item.on ? ' class="on"' : ''}${data}>${esc(item.label)}</button>`;
    })
    .join('');
  return (
    `<div class="seg" role="group"` +
    (opts.ariaLabel ? ` aria-label="${esc(opts.ariaLabel)}"` : '') +
    `>${buttons}</div>`
  );
}
