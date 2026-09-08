/**
 * The Profile page's verbs: the key rows, and the pack library.
 *
 * Doc 10 section 8 puts a key in the OS keychain, so nothing here keeps one.
 * Doc 10 section 9 has the core validate a pack file before anything else sees
 * it, so the page redraws from `profile.get` rather than from what the import
 * happened to return.
 */

import { RpcError } from '../../rpc.js';
import { COPY } from '../../strings.js';
import type { ControllerCtx } from './ctx.js';

export async function handle(action: string, el: HTMLElement, ctx: ControllerCtx): Promise<void> {
  switch (action) {
    case 'key':
      await setKey(el.closest<HTMLElement>('[data-key-ref]')?.dataset.keyRef ?? '', ctx);
      return;
    case 'pack': {
      const code = el.closest<HTMLElement>('[data-pack]')?.dataset.pack;
      if (code) await usePack(code, ctx);
      return;
    }
    case 'import-pack':
      await importPack(ctx);
  }
}

/**
 * Take a key and hand it to the keychain.
 *
 * `window.prompt` because this is the one input in the product whose value must
 * not survive anywhere: no element holds it, no state keeps it, and the only
 * place it goes is the core call below.
 */
async function setKey(keyRef: string, ctx: ControllerCtx): Promise<void> {
  if (!keyRef) return;
  const secret = window.prompt(`${COPY.profileKeyPrompt} ${keyRef}`);
  if (secret === null || !secret.trim()) return;
  try {
    await ctx.rpc.setKey(keyRef, secret);
    ctx.actions.toast(COPY.profileKeySavedToast);
    ctx.actions.keySaved();
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.profileKeyFailed, 'error');
  }
}

async function usePack(code: string, ctx: ControllerCtx): Promise<void> {
  try {
    await ctx.rpc.setPack(code);
    await ctx.rerender();
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
  }
}

async function importPack(ctx: ControllerCtx): Promise<void> {
  const path = ctx.body.querySelector<HTMLInputElement>('#pack-path')?.value ?? '';
  if (!path.trim()) return;
  try {
    const added = await ctx.rpc.importPack(path.trim());
    await ctx.rerender();
    ctx.actions.toast(`${COPY.profilePackImported} ${added.code}`);
  } catch (e) {
    ctx.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
  }
}
