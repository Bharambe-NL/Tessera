/**
 * What a page controller is handed, and what a page controller is.
 *
 * The Router owns navigation and one delegated listener over the page layer; a
 * controller owns what one page's elements do when they are clicked or
 * submitted. This is the seam between them: the core, the page state the render
 * reads, the shell callbacks the Router was built with, and the two hosts a
 * verb reads its form fields out of and draws its tool strip into.
 *
 * Every controller takes the same three arguments, so the Router's dispatch
 * table can name a controller without knowing anything about the page.
 */

import type { Rpc } from '../../rpc.js';
import type { PageState } from '../render.js';
import type { RouterActions, View } from '../router.js';

export interface ControllerCtx {
  /** The core, for the calls a verb makes. */
  rpc: Rpc;
  /** The state that survives a page switch, written where the render reads it. */
  state: PageState;
  /** The shell callbacks the Router was built with, such as openBoard. */
  actions: RouterActions;
  /** Move to another page, for a verb that lands the person somewhere else. */
  go(view: View): Promise<void>;
  /** Re-read and redraw whatever page is open. */
  rerender(): Promise<void>;
  /** The page body, which is where a form field is read from. */
  body: HTMLElement;
  /** The tool strip in the page header, for a partial redraw between reads. */
  tools: HTMLElement;
}

/**
 * One page's verbs behind one entry point.
 *
 * `action` is the name the Router's table gave the mark that matched, and `el`
 * is the element carrying it, so a controller reads the id or the verb it needs
 * from the element's own dataset rather than from a longer argument list.
 */
export type Controller = (action: string, el: HTMLElement, ctx: ControllerCtx) => Promise<void>;
