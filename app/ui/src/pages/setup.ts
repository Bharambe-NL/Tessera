/**
 * First run. Doc 11 section 6: choose a pack, add a model key, optionally a
 * folder.
 *
 * Doc 12 phase 11's acceptance is a fresh install to a first verified deep card
 * in under five minutes, so the shape of this screen is set by what a person
 * has to do before the product can answer anything, and nothing else. One of
 * the three steps is genuinely optional and says so; the other two are the
 * minimum, and neither is a preference.
 *
 * The key never comes back. Doc 10 section 8 puts it in the OS keychain, so the
 * input is cleared the moment it is handed over and the only thing this screen
 * can report afterwards is whether the keychain took it.
 */

import { esc } from '../canvas/visual.js';
import type { FirstRun } from '../rpc.js';
import { COPY } from '../strings.js';

/** What the screen is showing on top of what the core reported. */
export interface SetupState {
  run: FirstRun | null;
  /** Set once the key lands, so the step can show as finished. */
  keySaved: boolean;
  /** What the folder step added, and what reading it found. */
  folderAdded: { label: string; indexed: number; unreadable: number } | null;
  busy: boolean;
  error: string | null;
}

function step(n: number, title: string, done: boolean, body: string, note = ''): string {
  return (
    `<li class="step${done ? ' done' : ''}" data-step="${n}">` +
    `<h3><span class="n" aria-hidden="true">${done ? '✓' : n}</span>${esc(title)}</h3>` +
    (note ? `<p class="note">${esc(note)}</p>` : '') +
    `<div class="step-body">${body}</div>` +
    `</li>`
  );
}

export function setupHTML(state: SetupState): string {
  const run = state.run;
  if (!run) return `<p class="page-empty">${COPY.setupLoading}</p>`;

  const packs = run.packs
    .map(
      (code) =>
        `<button data-setup-pack="${esc(code)}"${code === run.active_pack ? ' class="on"' : ''}>` +
        `${esc(code)}</button>`,
    )
    .join('');

  // The key_ref the aliases want, shown so a person can see which entry they
  // are filling rather than pasting into an unlabelled box.
  const keyRef = run.key_refs[0] ?? '';
  const keyBody = run.has_key || state.keySaved
    ? `<p class="note">${COPY.setupKeyPresent}</p>`
    : `<form id="setup-key" class="setup-key">` +
      `<label for="setup-secret">${COPY.setupKeyLabel} <code>${esc(keyRef)}</code></label>` +
      `<input id="setup-secret" type="password" autocomplete="off" spellcheck="false" ` +
      `placeholder="${COPY.setupKeyPlaceholder}" aria-label="${COPY.setupKeyLabel}" />` +
      `<button type="submit" class="primary">${COPY.setupKeySave}</button>` +
      `</form>`;

  const folderBody = state.folderAdded
    ? `<p class="note">${COPY.setupFolderAdded} ${esc(state.folderAdded.label)}. ` +
      `${COPY.setupFolderIndexed} ${state.folderAdded.indexed}` +
      (state.folderAdded.unreadable > 0
        ? `. ${COPY.setupFolderUnreadable} ${state.folderAdded.unreadable}`
        : '') +
      `</p>`
    : `<form id="setup-folder" class="setup-folder">` +
      `<input id="setup-folder-root" placeholder="${COPY.setupFolderPath}" ` +
      `aria-label="${COPY.setupFolderPath}" autocomplete="off" />` +
      `<input id="setup-folder-label" placeholder="${COPY.setupFolderLabel}" ` +
      `aria-label="${COPY.setupFolderLabel}" autocomplete="off" />` +
      `<label class="check"><input id="setup-folder-sensitive" type="checkbox" /> ` +
      `${COPY.setupFolderSensitive}</label>` +
      `<button type="submit">${COPY.setupFolderAdd}</button>` +
      `</form>`;

  const ready = run.has_key || state.keySaved;

  return (
    `<h2>${COPY.setupTitle}</h2>` +
    `<p class="lede">${COPY.setupLede}</p>` +
    `<ol class="setup">` +
    step(1, COPY.setupPackTitle, true, `<div class="seg">${packs}</div>`, COPY.setupPackNote) +
    step(2, COPY.setupKeyTitle, ready, keyBody, COPY.setupKeyNote) +
    step(
      3,
      COPY.setupFolderTitle,
      state.folderAdded !== null,
      folderBody,
      COPY.setupFolderNote,
    ) +
    `</ol>` +
    (state.error ? `<p class="setup-error" role="alert">${esc(state.error)}</p>` : '') +
    (state.busy ? `<p class="page-empty">${COPY.setupWorking}</p>` : '') +
    `<div class="setup-acts">` +
    `<button id="setup-done" class="primary"${ready ? '' : ' disabled'}>${COPY.setupDone}</button>` +
    // Doc 11 section 9: an empty state instructs. Saying why the button is off
    // beats a person clicking a dead control and guessing.
    (ready ? '' : `<span class="note">${COPY.setupNeedsKey}</span>`) +
    `</div>`
  );
}
