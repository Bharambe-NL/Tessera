/**
 * The rail and the page it opens.
 *
 * Doc 11 section 5: "Main area is the canvas or a page (Home, Flags, Library,
 * Profile)". A page covers the canvas rather than replacing it, so the board
 * keeps its camera, its cards and its in flight run while a page is open, and
 * coming back is instant rather than a reload.
 *
 * Drawing a page lives in render.ts; what a page's elements do lives in
 * `controllers/`. What stays here is navigation, the state that survives a page
 * switch, and one delegated listener set over the whole page layer. The click
 * listener is a table: a mark on the element that was clicked names the page it
 * belongs to and the verb to call, and the router hands both to that page's
 * controller without knowing what the verb means.
 */

import { esc } from '../canvas/visual.js';
import type { Rpc } from '../rpc.js';
import { RpcError } from '../rpc.js';
import { COPY } from '../strings.js';
import type { Controller, ControllerCtx } from './controllers/ctx.js';
import * as flags from './controllers/flags.js';
import * as home from './controllers/home.js';
import * as library from './controllers/library.js';
import * as map from './controllers/map.js';
import * as notebook from './controllers/notebook.js';
import * as pages from './controllers/pages.js';
import * as profile from './controllers/profile.js';
import * as setup from './controllers/setup.js';
import { renderPage, type PageState } from './render.js';

/**
 * Setup is a view rather than a modal, so it uses the page layer the other four
 * use and inherits its focus handling and its escape. It is not on the rail:
 * nobody navigates to a first run, they arrive at one.
 */
export type View =
  'board' | 'home' | 'flags' | 'library' | 'notebook' | 'pages' | 'map' | 'profile' | 'setup';

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
  /**
   * Doc 17 section 6: open a board, start a lesson on this concept, and ask the
   * first check when the learner asked to be checked now.
   */
  startLesson(topic: string, check: boolean): Promise<void>;
  toast(message: string, level?: 'info' | 'warn' | 'error'): void;
  /** Leave the first run screen for the board. Doc 11 section 6. */
  finishSetup(): Promise<void>;
  /** A key was saved, so the composer's model choices may have grown. */
  keySaved(): void;
}

/** One row of the click table. */
interface Route {
  /** The selector that marks an element this route claims. */
  mark: string;
  /** The page whose controller owns it. */
  to: Controller;
  /** The verb handed to that controller. */
  action: string;
  /** Only while this page is open, for a mark two pages share. */
  on?: View;
}

/**
 * Every mark a page body can carry, in the order they are tried.
 *
 * Order is load bearing in one place: `data-concept` marks a Library row and a
 * Map node alike, so the Map's claim on it is qualified by the open view and
 * the Library's decision buttons are matched before it.
 */
const ROUTES: readonly Route[] = [
  { mark: '[data-setup-pack]', to: setup.handle, action: 'pack' },
  { mark: '#setup-done', to: setup.handle, action: 'done' },
  { mark: '[data-home-filter]', to: home.handle, action: 'filter' },
  { mark: '#home-create', to: home.handle, action: 'create' },
  { mark: '[data-board-act]', to: home.handle, action: 'board' },
  { mark: '[data-bulk]', to: flags.handle, action: 'bulk' },
  { mark: '[data-select-board]', to: flags.handle, action: 'select-board' },
  { mark: '[data-flag-act]', to: flags.handle, action: 'row' },
  { mark: '[data-library-tab]', to: library.handle, action: 'tab' },
  { mark: '[data-concept-act]', to: library.handle, action: 'decide' },
  { mark: '[data-key-act]', to: profile.handle, action: 'key' },
  { mark: '[data-notebook-act]', to: notebook.handle, action: 'turn' },
  { mark: '[data-page-act]', to: pages.handle, action: 'verb' },
  { mark: '[data-map-filter]', to: map.handle, action: 'filter' },
  { mark: '[data-concept]', to: map.handle, action: 'open-node', on: 'map' },
  { mark: '[data-map-act]', to: map.handle, action: 'verb' },
  { mark: '[data-place-skip]', to: map.handle, action: 'place-skip' },
  { mark: '[data-place-rate]', to: map.handle, action: 'place-rate' },
  { mark: '[data-map-rate]', to: map.handle, action: 'rate' },
  { mark: '[data-map-card]', to: map.handle, action: 'open-board' },
  { mark: '[data-map-page]', to: map.handle, action: 'open-page' },
  { mark: '[data-pack-act]', to: profile.handle, action: 'pack' },
];

/**
 * The forms, by id.
 *
 * Submit rather than click, so Enter works and the browser does not reload the
 * page out from under a half finished setup.
 */
const FORMS = new Map<string, { to: Controller; action: string }>([
  ['setup-key', { to: setup.handle, action: 'save-key' }],
  ['setup-folder', { to: setup.handle, action: 'watch-folder' }],
  ['pack-import', { to: profile.handle, action: 'import-pack' }],
  ['page-edit', { to: pages.handle, action: 'save' }],
  ['notebook-ask', { to: notebook.handle, action: 'ask' }],
]);

export class Router {
  view: View = 'board';
  private readonly state: PageState = {
    homeFilter: 'active',
    libraryTab: 'sources',
    pages: { open: null, editing: false },
    notebook: { session: null, asking: false },
    map: {
      map: null,
      open: null,
      links: null,
      filter: 'all',
      missionOnly: false,
      placing: null,
      skipped: [],
    },
    picked: new Set<string>(),
    confirmingDismiss: false,
    setup: {
      run: null,
      keySaved: false,
      folderAdded: null,
      busy: false,
      error: null,
    },
  };

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
    try {
      await renderPage(this.view, this.hosts, this.rpc, this.state, (n) => this.setFlagCount(n));
    } catch (e) {
      // A page that could not read says so rather than showing the last page's
      // rows under this page's title.
      this.hosts.body.innerHTML = `<p class="page-empty">${esc(
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
      const { flags: rows } = await this.rpc.flags();
      this.setFlagCount(rows.length);
    } catch {
      // The badge is a convenience; a failed read leaves the last count.
    }
  }

  /** Wire the rail and one delegated listener set over every page body. */
  attach(): void {
    this.hosts.rail.addEventListener('click', (e) => {
      const item = (e.target as HTMLElement | null)?.closest<HTMLElement>('.rail-item');
      const view = item?.dataset.view as View | undefined;
      if (view) void this.go(view);
    });

    this.hosts.page.addEventListener('click', (e) => {
      const target = e.target as HTMLElement | null;
      if (!target) return;
      for (const route of ROUTES) {
        if (route.on && route.on !== this.view) continue;
        const el = target.closest<HTMLElement>(route.mark);
        if (!el) continue;
        void route.to(route.action, el, this.ctx());
        return;
      }
    });

    this.hosts.page.addEventListener('keydown', (e) => flags.flagRowKeys(e, this.hosts.body));

    this.hosts.page.addEventListener('submit', (e) => {
      const form = e.target as HTMLElement | null;
      e.preventDefault();
      const route = FORMS.get(form?.id ?? '');
      if (form && route) void route.to(route.action, form, this.ctx());
    });

    // Row selection, which is a change rather than a click.
    this.hosts.page.addEventListener('change', (e) => {
      const box = e.target as HTMLInputElement | null;
      if (!box?.classList.contains('pick')) return;
      flags.pickChanged(box, this.ctx());
    });
  }

  /** What every controller is handed, built fresh so nothing outlives a click. */
  private ctx(): ControllerCtx {
    return {
      rpc: this.rpc,
      state: this.state,
      actions: this.actions,
      go: (view) => this.go(view),
      rerender: () => this.render(),
      body: this.hosts.body,
      tools: this.hosts.tools,
    };
  }
}
