/**
 * Every string the user reads, in one place.
 *
 * Two reasons it is a file rather than a habit. BN-024: the product name lives
 * in one constant, so changing it is one edit. BN-030: the house style lint in
 * `crates/tessera-style` reads TypeScript only from a file with this name,
 * because guessing which string in a general module is copy produced a six in
 * six false positive rate and a lint nobody would keep.
 *
 * So copy goes here and the lint checks it. `cargo test -p tessera-style` is
 * what enforces `HANDOFF.md` section 7 on everything below.
 *
 * The name is also declared in `app/src-tauri/tauri.conf.json` as the name the
 * operating system shows. That one is packaging metadata and cannot read a
 * TypeScript constant; the window title below is set from here at startup, so
 * the static title in `index.html` is only what shows before the script runs.
 */

export const PRODUCT_NAME = 'Tessera';

export const COPY = {
  /** Shown in the composer when no core is behind the page. */
  askOffline: `Open ${PRODUCT_NAME} to ask a question`,

  /** The error a call gets when the page is open outside the desktop app. */
  notConnected: `This page is not connected to a core. Open ${PRODUCT_NAME} to ask a question.`,
} as const;
