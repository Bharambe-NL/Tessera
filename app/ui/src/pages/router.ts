/**
 * The rail and the page it opens.
 *
 * Doc 11 section 5: "Main area is the canvas or a page (Home, Flags, Library,
 * Profile)". A page covers the canvas rather than replacing it, so the board
 * keeps its camera, its cards and its in flight run while a page is open, and
 * coming back is instant rather than a reload.
 *
 * Every page follows the same shape: a title, a tool strip in the header, and a
 * body rendered from one read. State that survives a page switch lives here,
 * because a filter or a tab the user chose should still be chosen when they
 * come back to it.
 */

import { esc } from '../canvas/visual.js';
import type { Rpc } from '../rpc.js';
import { RpcError } from '../rpc.js';
import { COPY } from '../strings.js';
import { flagsHTML, bulkHTML } from './flags.js';
import { homeHTML, homeToolsHTML, type HomeFilter } from './home.js';
import { conceptsHTML, libraryToolsHTML, sourcesHTML, type LibraryTab } from './library.js';
import { profileHTML, profileToolsHTML, type ProfileTab } from './profile.js';

export type View = 'board' | 'home' | 'flags' | 'library' | 'profile';

export interface RouterHosts {
  rail: HTMLElement;
  board: HTMLElement;
  page: HTMLElement;
  title: HTMLElement;
  tools: HTMLElement;
  body: HTMLElement;
  flagCount: HTMLElement;
}

export interface RouterActions {
  /** Open a board on the canvas and switch to it. */
  openBoard(boardId: string): Promise<void>;
  createBoard(): Promise<void>;
  /** Ask a question on the current board, for the Library ask verbs. */
  ask(question: string): void;
  toast(message: string, level?: 'info' | 'warn' | 'error'): void;
}

export class Router {
  view: View = 'board';
  private homeFilter: HomeFilter = 'active';
  private libraryTab: LibraryTab = 'sources';
  private profileTab: ProfileTab = 'context';
  /** Flags selected for a bulk decision, kept across a re-render. */
  private picked = new Set<string>();
  /** Doc 09 section 6: bulk Dismiss takes a second click with the count shown. */
  private confirmingDismiss = false;

  constructor(
    private readonly hosts: RouterHosts,
    private readonly rpc: Rpc,
    private readonly actions: RouterActions,
  ) {}

  async go(view: View): Promise<void> {
    this.view = view;
    for (const item of this.hosts.rail.querySelectorAll<HTMLElement>('.rail-item[data-view]')) {
      const on = item.dataset.view === view;
      item.classList.toggle('on', on);
      if (on) item.setAttribute('aria-current', 'page');
      else item.removeAttribute('aria-current');
    }

    this.hosts.page.hidden = view === 'board';
    this.hosts.board.classList.toggle('behind', view !== 'board');
    if (view === 'board') return;
    await this.render();
  }

  /** Re-read and redraw whatever page is open. */
  async render(): Promise<void> {
    const { title, tools, body } = this.hosts;
    try {
      switch (this.view) {
        case 'home': {
          title.textContent = COPY.railHome;
          tools.innerHTML = homeToolsHTML(this.homeFilter);
          const { boards } = await this.rpc.listBoards(this.homeFilter);
          body.innerHTML = homeHTML(boards, this.homeFilter);
          break;
        }
        case 'flags': {
          title.textContent = COPY.railFlags;
          const { flags } = await this.rpc.flags();
          // A flag decided elsewhere should not stay selected here.
          const live = new Set(flags.map((f) => f.id));
          for (const id of [...this.picked]) if (!live.has(id)) this.picked.delete(id);
          tools.innerHTML = bulkHTML(this.picked.size, this.confirmingDismiss);
          body.innerHTML = flagsHTML(flags, this.picked);
          this.setFlagCount(flags.length);
          break;
        }
        case 'library': {
          title.textContent = COPY.railLibrary;
          tools.innerHTML = libraryToolsHTML(this.libraryTab);
          if (this.libraryTab === 'sources') {
            const { sources } = await this.rpc.sources();
            body.innerHTML = sourcesHTML(sources);
          } else {
            const { concepts } = await this.rpc.concepts();
            body.innerHTML = conceptsHTML(concepts);
          }
          break;
        }
        case 'profile': {
          title.textContent = COPY.railProfile;
          tools.innerHTML = profileToolsHTML(this.profileTab);
          body.innerHTML = profileHTML(await this.rpc.profile(), this.profileTab);
          break;
        }
      }
    } catch (e) {
      // A page that could not read says so rather than showing the last page's
      // rows under this page's title.
      body.innerHTML = `<p class="page-empty">${esc(
        e instanceof RpcError ? e.message : COPY.pageUnread,
      )}</p>`;
    }
  }

  /** The rail badge, so an open flag is visible from the board. */
  setFlagCount(n: number): void {
    this.hosts.flagCount.textContent = String(n);
    this.hosts.flagCount.hidden = n === 0;
  }

  /** Read the count without opening the page, for the badge on boot. */
  async refreshFlagCount(): Promise<void> {
    try {
      const { flags } = await this.rpc.flags();
      this.setFlagCount(flags.length);
    } catch {
      // The badge is a convenience; a failed read leaves the last count.
    }
  }

  private async decide(ids: string[], decision: 'accept' | 'dismiss' | 'rerun'): Promise<void> {
    if (ids.length === 0) return;
    try {
      await this.rpc.decideFlags(ids, decision);
      for (const id of ids) this.picked.delete(id);
      this.confirmingDismiss = false;
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.flagsFailed, 'error');
    }
  }

  /** Wire the rail and one delegated listener over every page body. */
  attach(): void {
    this.hosts.rail.addEventListener('click', (e) => {
      const item = (e.target as HTMLElement | null)?.closest<HTMLElement>('.rail-item');
      const view = item?.dataset.view as View | undefined;
      if (view) void this.go(view);
    });

    this.hosts.page.addEventListener('click', (e) => {
      const target = e.target as HTMLElement | null;
      if (!target) return;

      // ---- Home
      const filter = target.closest<HTMLElement>('[data-home-filter]')?.dataset.homeFilter;
      if (filter === 'active' || filter === 'trashed') {
        this.homeFilter = filter;
        void this.render();
        return;
      }
      if (target.closest('#home-create')) {
        void this.actions.createBoard();
        return;
      }
      const boardVerb = target.closest<HTMLElement>('[data-board-act]');
      if (boardVerb) {
        void this.boardVerb(boardVerb.dataset.boardAct ?? '', boardVerb.dataset.board ?? '');
        return;
      }

      // ---- Flags
      const bulk = target.closest<HTMLElement>('[data-bulk]')?.dataset.bulk;
      if (bulk) {
        void this.bulk(bulk);
        return;
      }
      const selectBoard = target.closest<HTMLElement>('[data-select-board]');
      if (selectBoard) {
        for (const row of this.hosts.body.querySelectorAll<HTMLElement>(
          `.flag-group[data-board="${CSS.escape(selectBoard.dataset.selectBoard ?? '')}"] .flag-row`,
        )) {
          if (row.dataset.flag) this.picked.add(row.dataset.flag);
        }
        void this.render();
        return;
      }
      const flagVerb = target.closest<HTMLElement>('[data-flag-act]');
      if (flagVerb) {
        void this.flagVerb(flagVerb);
        return;
      }

      // ---- Library
      const libTab = target.closest<HTMLElement>('[data-library-tab]')?.dataset.libraryTab;
      if (libTab === 'sources' || libTab === 'concepts') {
        this.libraryTab = libTab;
        void this.render();
        return;
      }

      const conceptVerb = target.closest<HTMLElement>('[data-concept-act]');
      if (conceptVerb) {
        const id = conceptVerb.closest<HTMLElement>('[data-concept]')?.dataset.concept;
        const verb = conceptVerb.dataset.conceptAct;
        if (id && (verb === 'accept' || verb === 'dismiss')) {
          void this.decideConcept(id, verb === 'accept');
        }
        return;
      }

      // ---- Profile
      const profTab = target.closest<HTMLElement>('[data-profile-tab]')?.dataset.profileTab;
      if (profTab) {
        this.profileTab = profTab as ProfileTab;
        void this.render();
        return;
      }
      const keyVerb = target.closest<HTMLElement>('[data-key-act]');
      if (keyVerb) {
        void this.setKey(keyVerb.closest<HTMLElement>('[data-key-ref]')?.dataset.keyRef ?? '');
      }
    });

    // Doc 09 section 14: flag rows navigable with arrows. Up and down move
    // between rows; Home and End go to the ends; Space picks the row for a bulk
    // decision without reaching for its checkbox.
    this.hosts.page.addEventListener('keydown', (e) => {
      const row = (e.target as HTMLElement | null)?.closest<HTMLElement>('.flag-row');
      if (!row) return;

      const rows = [...this.hosts.body.querySelectorAll<HTMLElement>('.flag-row')];
      const at = rows.indexOf(row);
      if (at < 0) return;

      let next = at;
      switch (e.key) {
        case 'ArrowDown':
          next = Math.min(at + 1, rows.length - 1);
          break;
        case 'ArrowUp':
          next = Math.max(at - 1, 0);
          break;
        case 'Home':
          next = 0;
          break;
        case 'End':
          next = rows.length - 1;
          break;
        case ' ': {
          const box = row.querySelector<HTMLInputElement>('.pick');
          if (!box) return;
          e.preventDefault();
          box.checked = !box.checked;
          box.dispatchEvent(new Event('change', { bubbles: true }));
          return;
        }
        default:
          return;
      }
      // Off the end, focus stays where it is rather than wrapping into nothing.
      e.preventDefault();
      rows[next]?.focus();
    });

    // Row selection, which is a change rather than a click.
    this.hosts.page.addEventListener('change', (e) => {
      const box = e.target as HTMLInputElement | null;
      if (!box?.classList.contains('pick')) return;
      const id = box.closest<HTMLElement>('[data-flag]')?.dataset.flag;
      if (!id) return;
      if (box.checked) this.picked.add(id);
      else this.picked.delete(id);
      this.confirmingDismiss = false;
      this.hosts.tools.innerHTML = bulkHTML(this.picked.size, this.confirmingDismiss);
    });
  }

  private async boardVerb(verb: string, boardId: string): Promise<void> {
    if (!boardId) return;
    try {
      switch (verb) {
        case 'open':
          await this.actions.openBoard(boardId);
          return;
        case 'trash':
          await this.rpc.trashBoard(boardId);
          break;
        case 'restore':
          await this.rpc.restoreBoard(boardId);
          break;
        case 'purge':
          await this.rpc.purgeBoard(boardId);
          break;
        default:
          return;
      }
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.boardVerbFailed, 'error');
    }
  }

  private async flagVerb(button: HTMLElement): Promise<void> {
    const row = button.closest<HTMLElement>('[data-flag]');
    const id = row?.dataset.flag;
    if (!id) return;

    const verb = button.dataset.flagAct;
    if (verb === 'open') {
      // Doc 09 section 5's Open on a flag: go to the card.
      const boardId = row?.closest<HTMLElement>('.flag-group')?.dataset.board;
      if (boardId) await this.actions.openBoard(boardId);
      return;
    }
    if (verb === 'accept' || verb === 'dismiss' || verb === 'rerun') {
      await this.decide([id], verb);
    }
  }

  private async bulk(action: string): Promise<void> {
    const ids = [...this.picked];
    switch (action) {
      case 'accept':
        // Doc 09 section 6: bulk Accept needs no confirmation, because the flag
        // stands and the content stays hidden.
        await this.decide(ids, 'accept');
        return;
      case 'dismiss':
        // The first click asks; the second decides.
        this.confirmingDismiss = true;
        this.hosts.tools.innerHTML = bulkHTML(this.picked.size, true);
        return;
      case 'dismiss-confirm':
        await this.decide(ids, 'dismiss');
        return;
      case 'clear':
        this.picked.clear();
        this.confirmingDismiss = false;
        await this.render();
    }
  }

  private async decideConcept(conceptId: string, accept: boolean): Promise<void> {
    try {
      await this.rpc.decideConcept(conceptId, accept);
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.conceptFailed, 'error');
    }
  }

  /**
   * Take a key and hand it to the keychain.
   *
   * `window.prompt` because this is the one input in the product whose value
   * must not survive anywhere: no element holds it, no state keeps it, and the
   * only place it goes is the core call below.
   */
  private async setKey(keyRef: string): Promise<void> {
    if (!keyRef) return;
    const secret = window.prompt(`${COPY.profileKeyPrompt} ${keyRef}`);
    if (secret === null || !secret.trim()) return;
    try {
      await this.rpc.setKey(keyRef, secret);
      this.actions.toast(COPY.profileKeySavedToast);
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.profileKeyFailed, 'error');
    }
  }
}
