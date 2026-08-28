/**
 * The chip: one shape, four hues, a meaning per class.
 *
 * The classes are the vocabulary components.css maps onto the node hues
 * (`ok`, `sev warn`, `status-proposed`, `grounded`, …), so a call site names
 * what the chip means and never what colour it is.
 */

import { esc } from '../canvas/visual.js';

export function chip(label: string, opts: { classes?: string; title?: string } = {}): string {
  return (
    `<span class="chip${opts.classes ? ` ${esc(opts.classes)}` : ''}"` +
    (opts.title ? ` title="${esc(opts.title)}"` : '') +
    `>${esc(label)}</span>`
  );
}
