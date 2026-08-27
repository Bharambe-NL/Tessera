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

  // ------------------------------------------------------- the card header --

  /** A follow-up card has no anchor to name, so the kind is the title. */
  followTitle: 'Follow-up',
  /** Doc 09 section 4: the confidence dot before the Verifier has run. */
  unverified: 'Unverified',
  /** Hover on the dot once it has. The number follows this word. */
  confidence: 'Confidence',
  /** Hover on the model alias, doc 09 section 4. */
  rerunAs: 'Rerun as…',
  /** Doc 09 section 5's Rerun verb on a card. */
  rerunCard: 'Check this card again',
  /** The flag chip opens the card list of flags rather than leaving a count. */
  openFlags: 'Show what was flagged',
  closeFlags: 'Hide what was flagged',

  // --------------------------------------------------------- the card body --

  /** Read aloud while a card is running and its stage list is still empty. */
  working: 'Working',
  cardFailed: 'This card did not finish. Rerun it, or open how this was built.',
  keyFindings: 'Key findings',
  howBuilt: 'How this was built',
  /** The count follows this word. */
  sources: 'Sources',
  /** The marker on a citation whose source moved under the card. Doc 07 B3. */
  staleTag: 'stale',
  /** Doc 07 section B8.3: a block that fails is hidden, never removed. */
  blockHidden: 'Hidden after review.',
  blockHiddenUnexplained: 'A flag covers this block.',

  // ------------------------------------------------------ the card actions --

  askFollowUp: 'Ask a follow-up',
  sendFollowUp: 'Send follow-up',

  // ------------------------------------------------------------- the shell --

  modeStarting: 'Starting',
  modeLive: 'Live',
  modeWorking: 'Working',
  modeOffline: 'Offline',

  // -------------------------------------------------------------- failures --

  /** Doc 11 section 9: say what happened and how to fix it. */
  askFailed: 'That card did not finish.',
  coreSilent: 'The core did not answer.',
  rerunFailed: 'That card could not be checked again.',
  renameFailed: 'That board could not be renamed.',
} as const;
