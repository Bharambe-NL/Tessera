/**
 * Profile. Doc 11 section 6 named five tabs; the owner asked on 2026-08-30 for
 * one page instead, with the same five as sections a scroll passes through.
 * Nothing here is worth a navigation of its own.
 *
 * The Models section is what retires the `tessera-keys` CLI. It never shows a
 * key and never receives one back from the core: doc 10 section 8 puts the
 * secret in the OS keychain, so what a row can say is which key_ref an alias
 * wants and whether the keychain has it.
 */

import { esc } from '../canvas/visual.js';
import type { ProfileSummary } from '../rpc.js';
import { COPY } from '../strings.js';
import { button } from '../ui/button.js';
import { chip } from '../ui/chip.js';
import { emptyState } from './shared.js';

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
          chip(a.provider) +
          chip(a.key_present ? COPY.profileKeySaved : COPY.profileKeyMissing, {
            classes: a.key_present ? 'ok' : 'missing',
          }) +
          `</div>` +
          `<p class="meta">${esc(a.model)}, ${COPY.profileKeyRef} ${esc(a.key_ref)}</p></div>` +
          `<div class="verbs">` +
          button(a.key_present ? COPY.profileKeyReplace : COPY.profileKeyAdd, {
            data: { 'key-act': 'edit' },
          }) +
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
          // Doc 05 section 10 separates these two, and so does this row.
          chip(r.configured ? COPY.profileConfigured : COPY.profileUnconfigured, {
            classes: r.configured ? 'ok' : 'missing',
          }) +
          (r.enabled_by_default ? chip(COPY.profileOnByDefault) : '') +
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
          chip(p.built_in ? COPY.profilePackBuiltIn : COPY.profilePackImported) +
          (p.active ? chip(COPY.profilePackActive, { classes: 'ok' }) : '') +
          `</div></div>` +
          (p.active
            ? ''
            : `<div class="verbs">${button(COPY.profilePackUse, { data: { 'pack-act': 'use' } })}</div>`) +
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
    button(COPY.profilePackImport, { submit: true }) +
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

function section(id: string, heading: string, body: string): string {
  return (
    `<section class="profile-section" data-profile-section="${esc(id)}">` +
    `<h2>${esc(heading)}</h2>${body}</section>`
  );
}

export function profileHTML(profile: ProfileSummary | null): string {
  if (!profile) return emptyState(COPY.profileUnread);

  const context = facts([
    [COPY.profileId, profile.profile_id],
    [COPY.profileProvider, profile.provider],
    [COPY.profileActivePack, profile.active_pack],
  ]);

  return (
    section('context', COPY.profileContext, context) +
    section('models', COPY.profileModels, models(profile)) +
    section('retrievers', COPY.profileRetrievers, retrievers(profile)) +
    section('doctrine', COPY.profileDoctrine, doctrine(profile)) +
    section('diagnostics', COPY.profileDiagnostics, diagnostics(profile))
  );
}
