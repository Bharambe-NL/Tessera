/**
 * First run's verbs. Doc 11 section 6.
 *
 * Three steps, one shape: each one goes busy, does its call, and redraws with
 * whatever it learned, so an error stays where the person is looking rather
 * than in a toast that has scrolled away by the time they read the step it
 * belongs to. The folder step reads the shared form in `pages/forms/folder.ts`.
 */

import { RpcError } from '../../rpc.js';
import { COPY } from '../../strings.js';
import { readFolderForm } from '../forms/folder.js';
import type { ControllerCtx } from './ctx.js';

export async function handle(action: string, el: HTMLElement, ctx: ControllerCtx): Promise<void> {
  switch (action) {
    case 'pack': {
      const code = el.dataset.setupPack;
      if (!code) return;
      await setupStep(() => ctx.rpc.setPack(code).then(() => undefined), ctx);
      return;
    }
    case 'done':
      await ctx.actions.finishSetup();
      return;
    case 'save-key':
      await saveKey(ctx);
      return;
    case 'watch-folder':
      await watchFolder(ctx);
  }
}

async function saveKey(ctx: ControllerCtx): Promise<void> {
  const input = ctx.body.querySelector<HTMLInputElement>('#setup-secret');
  const secret = input?.value.trim() ?? '';
  if (!secret) return;
  const keyRef = ctx.state.setup.run?.key_refs[0] ?? '';
  // Cleared before the call rather than after. The field holds the only copy of
  // the secret in this process, and a failed call leaves the screen up.
  if (input) input.value = '';
  await setupStep(async () => {
    await ctx.rpc.setKey(keyRef, secret);
    ctx.state.setup.keySaved = true;
    ctx.actions.keySaved();
  }, ctx);
}

async function watchFolder(ctx: ControllerCtx): Promise<void> {
  const form = readFolderForm(ctx.body);
  if (!form.root) return;
  await setupStep(async () => {
    const added = await ctx.rpc.watchFolder(form);
    ctx.state.setup.folderAdded = {
      label: added.label,
      indexed: added.indexed,
      unreadable: added.errors.length,
    };
  }, ctx);
}

/**
 * Run one setup step and redraw, keeping the error where the person is looking
 * rather than in a toast.
 */
async function setupStep(work: () => Promise<void>, ctx: ControllerCtx): Promise<void> {
  ctx.state.setup.busy = true;
  ctx.state.setup.error = null;
  await ctx.rerender();
  try {
    await work();
  } catch (e) {
    ctx.state.setup.error = e instanceof RpcError ? e.message : COPY.pageUnread;
  }
  ctx.state.setup.busy = false;
  await ctx.rerender();
}
