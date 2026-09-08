/**
 * The watched folder form, written once and read once.
 *
 * Doc 10 section 9: a watched folder is a root, a label, and whether what is
 * inside it counts as sensitive. First run asks for one as its third step, and
 * the Profile page asks for another later. Two copies of three input ids would
 * be two forms that drift, and the reader below is the half that would drift
 * first, because it names the ids the writer chose.
 */

import { COPY } from '../../strings.js';
import { button } from '../../ui/button.js';

/** What a person typed, trimmed, with the label falling back to the root. */
export interface FolderForm {
  root: string;
  label: string;
  sensitive: boolean;
}

export function folderFormHTML(): string {
  return (
    `<form id="setup-folder" class="setup-folder">` +
    `<input id="setup-folder-root" placeholder="${COPY.setupFolderPath}" ` +
    `aria-label="${COPY.setupFolderPath}" autocomplete="off" />` +
    `<input id="setup-folder-label" placeholder="${COPY.setupFolderLabel}" ` +
    `aria-label="${COPY.setupFolderLabel}" autocomplete="off" />` +
    `<label class="check"><input id="setup-folder-sensitive" type="checkbox" /> ` +
    `${COPY.setupFolderSensitive}</label>` +
    button(COPY.setupFolderAdd, { submit: true }) +
    `</form>`
  );
}

/**
 * Read the form out of the page body.
 *
 * An empty root comes back empty rather than raising: the caller decides what
 * an unfilled form means, and on first run it means the person pressed the
 * button before typing anything.
 */
export function readFolderForm(body: HTMLElement): FolderForm {
  const root = (body.querySelector<HTMLInputElement>('#setup-folder-root')?.value ?? '').trim();
  const label = (body.querySelector<HTMLInputElement>('#setup-folder-label')?.value ?? '').trim();
  const sensitive =
    body.querySelector<HTMLInputElement>('#setup-folder-sensitive')?.checked ?? false;
  return { root, label: label || root, sensitive };
}
