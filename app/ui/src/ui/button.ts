/**
 * The one way a button is written.
 *
 * Doc 11 section 4: hand built components, shared primitives only. Every page
 * was writing its own `<button>` string, and they disagreed on escaping, on
 * classes and on whether a type attribute exists. This is the agreement:
 * `.btn` from components.css, a variant when the verb warrants one, and every
 * value escaped on the way in.
 */

import { esc } from '../canvas/visual.js';

export interface ButtonOpts {
  /**
   * 'primary' for the one action a view leads with; 'danger' for a
   * destructive verb; 'quiet' for a dismissal or an icon. Absent is the
   * workhorse bordered button.
   */
  variant?: 'primary' | 'danger' | 'quiet';
  id?: string;
  disabled?: boolean;
  /** Submits the form it sits in; everything else is a plain button. */
  submit?: boolean;
  ariaLabel?: string;
  /** data-* attributes, keys given without the data- prefix. */
  data?: Record<string, string>;
  /** Extra classes a call site's CSS or a test hangs off. */
  classes?: string;
}

export function button(label: string, opts: ButtonOpts = {}): string {
  const cls = ['btn', opts.variant, opts.classes].filter(Boolean).join(' ');
  const data = Object.entries(opts.data ?? {})
    .map(([key, value]) => ` data-${key}="${esc(value)}"`)
    .join('');
  return (
    `<button type="${opts.submit ? 'submit' : 'button'}" class="${cls}"` +
    (opts.id ? ` id="${esc(opts.id)}"` : '') +
    (opts.ariaLabel ? ` aria-label="${esc(opts.ariaLabel)}"` : '') +
    (opts.disabled ? ' disabled' : '') +
    `${data}>${esc(label)}</button>`
  );
}
