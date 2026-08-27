/**
 * Profile. Doc 11 section 6: Context, Models, Retrievers, Doctrine,
 * Diagnostics.
 *
 * The Models section is what retires the `tessera-keys` CLI. It never shows a
 * key and never receives one back from the core: doc 10 section 8 puts the
 * secret in the OS keychain, so what a row can say is which key_ref an alias
 * wants and whether the keychain has it.
 */

import { esc } from '../canvas/visual.js';
import type { ProfileSummary } from '../rpc.js';
import { COPY } from '../strings.js';
import { emptyState } from './shared.js';

export type ProfileTab = 'context' | 'models' | 'retrievers' | 'doctrine' | 'diagnostics';

const TABS: { id: ProfileTab; label: () => string }[] = [
  { id: 'context', label: () => COPY.profileContext },
  { id: 'models', label: () => COPY.profileModels },
  { id: 'retrievers', label: () => COPY.profileRetrievers },
  { id: 'doctrine', label: () => COPY.profileDoctrine },
  { id: 'diagnostics', label: () => COPY.profileDiagnostics },
];

export function profileToolsHTML(tab: ProfileTab): string {
  return (
    `<div class="seg" role="group" aria-label="${COPY.profileTabsLabel}">` +
    TABS.map(
      (t) => `<button data-profile-tab="${t.id}"${t.id === tab ? ' class="on"' : ''}>${t.label()}</button>`,
    ).join('') +
    `</div>`
  );
}

function facts(rows: [string, string][]): string {
  return (
    `<dl class="facts">` +
    rows
      .map(([term, detail]) => `<div><dt>${esc(term)}</dt><dd>${esc(detail)}</dd></div>`)
      .join('') +
    `</dl>`
  );
}

function models(profile: ProfileSummary): string {
  const aliases = profile.aliases ?? [];
  if (aliases.length === 0) return emptyState(COPY.profileNoAliases);
  return (
    `<ul class="lib-list">` +
    aliases
      .map(
        (a) =>
          `<li class="lib-row" data-key-ref="${esc(a.key_ref)}">` +
          `<div class="what"><div class="line">` +
          `<span class="title">${esc(a.alias)}</span>` +
          `<span class="chip">${esc(a.provider)}</span>` +
          `<span class="chip ${a.key_present ? 'ok' : 'missing'}">` +
          `${a.key_present ? COPY.profileKeySaved : COPY.profileKeyMissing}</span>` +
          `</div>` +
          `<p class="meta">${esc(a.model)}, ${COPY.profileKeyRef} ${esc(a.key_ref)}</p></div>` +
          `<div class="verbs">` +
          `<button data-key-act="edit">${a.key_present ? COPY.profileKeyReplace : COPY.profileKeyAdd}</button>` +
          `</div></li>`,
      )
      .join('') +
    `</ul>` +
    // Doc 10 section 8, stated where a person is about to paste a secret.
    `<p class="page-note">${COPY.profileKeyNotice}</p>`
  );
}

function retrievers(profile: ProfileSummary): string {
  const rows = profile.retrievers ?? [];
  if (rows.length === 0) return emptyState(COPY.profileNoRetrievers);
  return (
    `<ul class="lib-list">` +
    rows
      .map(
        (r) =>
          `<li class="lib-row"><div class="what"><div class="line">` +
          `<span class="title">${esc(r.id)}</span>` +
          `<span class="chip ${r.configured ? 'ok' : 'missing'}">` +
          // Doc 05 section 10 separates these two, and so does this row.
          `${r.configured ? COPY.profileConfigured : COPY.profileUnconfigured}</span>` +
          (r.enabled_by_default ? `<span class="chip">${COPY.profileOnByDefault}</span>` : '') +
          `</div></div></li>`,
      )
      .join('') +
    `</ul>`
  );
}

/**
 * Doctrine. Doc 11 section 6 and doc 10 section 9.
 *
 * The list says which packs ship with the app and which came from a file,
 * because they are not the same claim: a shipped pack is the same on every
 * machine and an imported one is whatever its author wrote.
 */
function doctrine(profile: ProfileSummary): string {
  const packs = profile.pack_details ?? profile.packs.map((code) => ({
    code,
    built_in: true,
    active: code === profile.active_pack,
  }));
  const problems = profile.pack_problems ?? [];

  return (
    `<ul class="lib-list">` +
    packs
      .map(
        (p) =>
          `<li class="lib-row" data-pack="${esc(p.code)}"><div class="what"><div class="line">` +
          `<span class="title">${esc(p.code)}</span>` +
          `<span class="chip">${p.built_in ? COPY.profilePackBuiltIn : COPY.profilePackImported}</span>` +
          (p.active ? `<span class="chip ok">${COPY.profilePackActive}</span>` : '') +
          `</div></div>` +
          (p.active
            ? ''
            : `<div class="verbs"><button data-pack-act="use">${COPY.profilePackUse}</button></div>`) +
          `</li>`,
      )
      .join('') +
    `</ul>` +
    // A file that did not load is said here, where the fix is, rather than in a
    // log. The profile opened without it.
    (problems.length > 0
      ? `<p class="page-note" role="alert">${COPY.profilePackUnread}</p>` +
        `<ul class="lib-list">` +
        problems
          .map(
            (p) =>
              `<li class="lib-row"><div class="what"><div class="line">` +
              `<span class="title">${esc(p.file)}</span></div>` +
              `<p class="meta">${esc(p.detail)}</p></div></li>`,
          )
          .join('') +
        `</ul>`
      : '') +
    `<form id="pack-import" class="setup-folder">` +
    `<input id="pack-path" placeholder="${COPY.profilePackPath}" ` +
    `aria-label="${COPY.profilePackPath}" autocomplete="off" />` +
    `<button type="submit">${COPY.profilePackImport}</button>` +
    `</form>` +
    `<p class="page-note">${COPY.profilePackImportNote}</p>`
  );
}

function diagnostics(profile: ProfileSummary): string {
  const d = profile.diagnostics;
  if (!d) return emptyState(COPY.profileNoDiagnostics);
  return facts([
    [COPY.profileBoards, String(d.boards)],
    [COPY.profileTrashed, String(d.boards_trashed)],
    [COPY.profileCards, String(d.cards)],
    [COPY.profileOpenFlags, String(d.open_flags)],
    [COPY.profileSources, `${d.sources} (${d.sources_stale} ${COPY.staleTag})`],
    [COPY.profileConcepts, String(d.concepts)],
    [COPY.profileEvents, String(d.events)],
  ]);
}

export function profileHTML(profile: ProfileSummary | null, tab: ProfileTab): string {
  if (!profile) return emptyState(COPY.profileUnread);

  switch (tab) {
    case 'models':
      return models(profile);
    case 'retrievers':
      return retrievers(profile);
    case 'doctrine':
      return doctrine(profile);
    case 'diagnostics':
      return diagnostics(profile);
    default:
      return facts([
        [COPY.profileId, profile.profile_id],
        [COPY.profileProvider, profile.provider],
        [COPY.profileActivePack, profile.active_pack],
      ]);
  }
}
