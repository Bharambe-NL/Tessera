/**
 * The `__TAURI__` shim.
 *
 * The UI reaches the core through `window.__TAURI__.core.invoke('rpc', …)`,
 * which the Tauri webview injects and a browser does not have. `rpc.ts` reads
 * its absence as "no core behind the page" and disables the composer, so a test
 * without this shim measures the offline fixture and nothing else.
 *
 * This installs the same two commands the shell registers, pointed at the dev
 * server's `/rpc`. It runs before any page script, so `new Rpc()` finds it.
 */

import type { Page } from '@playwright/test';

/**
 * Give the dev server a core with no boards on it.
 *
 * `boot()` opens `boards[0]` and creates one only when there is none, so
 * without this every test after the first lands on the board its predecessor
 * filled and counts that one's cards. Test surface on a test binary; the shell
 * has no such call and never will.
 */
export async function freshCore(page: Page): Promise<void> {
  const response = await page.request.post('/reset');
  if (!response.ok()) throw new Error(`the dev server would not reset: ${response.status()}`);
}

/**
 * A core with nothing in its keychain, which is what a fresh install is.
 *
 * `freshCore` seeds a key, because every other test needs a core that can
 * answer. The first run screen exists for the state where there is none, and a
 * test that used the seeded core would drive a screen the product would never
 * have shown.
 */
export async function keylessCore(page: Page): Promise<void> {
  const response = await page.request.post('/reset?keyless=1');
  if (!response.ok()) throw new Error(`the dev server would not reset: ${response.status()}`);
}

/**
 * A core with the boards retriever on, which is what memory is.
 *
 * Off in the other resets, because memory adds an `own_card` source to a board
 * that had none and the Library counts sources. Doc 15 section 3 makes a card
 * eligible to be remembered only at deep or research, so a test using this has
 * to ask at deep.
 */
/**
 * A core with a vault in it, and the corpus behind it.
 *
 * Doc 16 section 3.4: a notebook question reads the vault. The page answers the
 * same question the corpus document does, which is why it is its own flag: a
 * card resting on it is flagged for page sole support, and a flagged card is
 * not remembered, so folding it into the memory fixture would break the premise
 * doc 15's own test is built on.
 */
export async function vaultCore(page: Page): Promise<void> {
  const response = await page.request.post('/reset?memory=1&vault=1');
  if (!response.ok()) throw new Error(`the dev server would not reset: ${response.status()}`);
}

export async function memoryCore(page: Page): Promise<void> {
  const response = await page.request.post('/reset?memory=1');
  if (!response.ok()) throw new Error(`the dev server would not reset: ${response.status()}`);
}

export async function useCore(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const invoke = async (cmd: string, args: Record<string, unknown>): Promise<unknown> => {
      if (cmd === 'rpc') {
        const response = await fetch('/rpc', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: String(args.request),
        });
        return await response.text();
      }
      // `report_gate` and `report_gate_error` are the perf gate's reporting
      // path. Accepting them keeps `?gate=` runnable here too.
      return null;
    };
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = { core: { invoke } };
  });
}

/** How many cards the canvas is currently showing. */
export async function cardCount(page: Page): Promise<number> {
  return await page.locator('#cards .card').count();
}
