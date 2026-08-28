/**
 * The rail and the page it opens.
 *
 * Doc 11 section 5: "Main area is the canvas or a page (Home, Flags, Library,
 * Profile)". A page covers the canvas rather than replacing it, so the board
 * keeps its camera, its cards and its in flight run while a page is open, and
 * coming back is instant rather than a reload.
 *
 * Drawing a page lives in render.ts; what stays here is navigation, the state
 * that survives a page switch, and the verbs a page's elements carry.
 */

import { esc } from '../canvas/visual.js';
import type { Rpc } from '../rpc.js';
import { RpcError } from '../rpc.js';
import { COPY } from '../strings.js';
import { bulkHTML } from './flags.js';
import type { MapFilter } from './map.js';
import type { ProfileTab } from './profile.js';
import { renderPage, type PageState } from './render.js';

/**
 * Setup is a view rather than a modal, so it uses the page layer the other four
 * use and inherits its focus handling and its escape. It is not on the rail:
 * nobody navigates to a first run, they arrive at one.
 */
export type View =
  | 'board'
  | 'home'
  | 'flags'
  | 'library'
  | 'notebook'
  | 'pages'
  | 'map'
  | 'profile'
  | 'setup';

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
}

export class Router {
  view: View = 'board';
  private readonly state: PageState = {
    homeFilter: 'active',
    libraryTab: 'sources',
    profileTab: 'context',
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
      const { flags } = await this.rpc.flags();
      this.setFlagCount(flags.length);
    } catch {
      // The badge is a convenience; a failed read leaves the last count.
    }
  }

  // ------------------------------------------------------------------ map --

  /** Doc 17 section 6's node panel, with its links read as it opens. */
  private async openNode(conceptId: string): Promise<void> {
    const concept = this.state.map.map?.concepts.find((c) => c.concept_id === conceptId);
    if (!concept) return;
    this.state.map.open = concept;
    this.state.map.links = null;
    await this.render();
    try {
      this.state.map.links = await this.rpc.mapConcept(conceptId);
      await this.render();
    } catch {
      // The panel stands without its links rather than closing: the rating and
      // the three verbs are what a learner came for, and they need no read.
    }
  }

  private async mapVerb(verb: string): Promise<void> {
    if (verb === 'close') {
      this.state.map.open = null;
      this.state.map.links = null;
      await this.render();
      return;
    }
    if (verb === 'mission') {
      this.state.map.missionOnly = !this.state.map.missionOnly;
      await this.render();
      return;
    }
    if (verb === 'place' || verb === 'placed') {
      this.state.map.placing = verb === 'place';
      // Going back into placement brings the skipped tiles with it: a skip is
      // "not now" rather than "never ask me", and the learner asked again.
      if (verb === 'place') this.state.map.skipped = [];
      await this.render();
      return;
    }

    const concept = this.state.map.open;
    if (!concept) return;
    try {
      // Doc 17 section 6's three verbs, each of which lands the learner on a
      // board. A lesson and a check both start a session; the check asks its
      // first question straight away, which is what "check me now" means.
      if (verb === 'explore') {
        const { board_id } = await this.rpc.createBoard(concept.term);
        await this.actions.openBoard(board_id);
        this.actions.ask(concept.term);
        return;
      }
      if (verb === 'lesson' || verb === 'check') {
        await this.actions.startLesson(concept.term, verb === 'check');
      }
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.mapFailed, 'error');
    }
  }

  /** Doc 17 section 3: one tile of the placement pass, rated or passed over. */
  private async place(conceptId: string, rating: number | null): Promise<void> {
    if (rating === null) {
      this.state.map.skipped = [...this.state.map.skipped, conceptId];
      await this.render();
      return;
    }
    try {
      await this.rpc.rateConcept(conceptId, rating);
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.mapFailed, 'error');
    }
  }

  /** Doc 17 section 2.1: the learner rates, and it is a claim, never evidence. */
  private async rate(rating: number): Promise<void> {
    const concept = this.state.map.open;
    if (!concept || !Number.isFinite(rating)) return;
    try {
      await this.rpc.rateConcept(concept.concept_id, rating);
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.mapFailed, 'error');
    }
  }

  private async openPageFromMap(pageId: string): Promise<void> {
    try {
      this.state.pages = { open: await this.rpc.page(pageId), editing: false };
      await this.go('pages');
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
    }
  }

  private async decide(ids: string[], decision: 'accept' | 'dismiss' | 'rerun'): Promise<void> {
    if (ids.length === 0) return;
    try {
      await this.rpc.decideFlags(ids, decision);
      for (const id of ids) this.state.picked.delete(id);
      this.state.confirmingDismiss = false;
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

      // ---- First run
      const pack = target.closest<HTMLElement>('[data-setup-pack]')?.dataset.setupPack;
      if (pack) {
        void this.setupPack(pack);
        return;
      }
      if (target.closest('#setup-done')) {
        void this.actions.finishSetup();
        return;
      }

      // ---- Home
      const filter = target.closest<HTMLElement>('[data-home-filter]')?.dataset.homeFilter;
      if (filter === 'active' || filter === 'trashed') {
        this.state.homeFilter = filter;
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
          if (row.dataset.flag) this.state.picked.add(row.dataset.flag);
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
        this.state.libraryTab = libTab;
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
        this.state.profileTab = profTab as ProfileTab;
        void this.render();
        return;
      }
      const keyVerb = target.closest<HTMLElement>('[data-key-act]');
      if (keyVerb) {
        void this.setKey(keyVerb.closest<HTMLElement>('[data-key-ref]')?.dataset.keyRef ?? '');
        return;
      }
      // ---- Notebook
      const notebookVerb = target.closest<HTMLElement>('[data-notebook-act]');
      if (notebookVerb) {
        const turn = notebookVerb.closest<HTMLElement>('[data-card]');
        void this.notebookVerb(notebookVerb.dataset.notebookAct ?? '', turn?.dataset.card);
        return;
      }

      // ---- Pages
      const pageVerb = target.closest<HTMLElement>('[data-page-act]');
      if (pageVerb) {
        const row = pageVerb.closest<HTMLElement>('[data-page]');
        void this.pageVerb(
          pageVerb.dataset.pageAct ?? '',
          row?.dataset.page,
          pageVerb.dataset.title,
        );
        return;
      }

      // ---- Map
      const mapFilter = target.closest<HTMLElement>('[data-map-filter]')?.dataset.mapFilter;
      if (mapFilter) {
        this.state.map.filter = mapFilter as MapFilter;
        void this.render();
        return;
      }
      const node = target.closest<HTMLElement>('[data-concept]');
      if (node && this.view === 'map') {
        void this.openNode(node.dataset.concept ?? '');
        return;
      }
      const mapVerb = target.closest<HTMLElement>('[data-map-act]')?.dataset.mapAct;
      if (mapVerb) {
        void this.mapVerb(mapVerb);
        return;
      }
      const skip = target.closest<HTMLElement>('[data-place-skip]')?.dataset.placeSkip;
      if (skip) {
        void this.place(skip, null);
        return;
      }
      const tile = target.closest<HTMLElement>('[data-place-rate]');
      if (tile?.dataset.placeConcept) {
        void this.place(tile.dataset.placeConcept, Number(tile.dataset.placeRate));
        return;
      }
      const rate = target.closest<HTMLElement>('[data-map-rate]')?.dataset.mapRate;
      if (rate !== undefined) {
        void this.rate(Number(rate));
        return;
      }
      const mapCard = target.closest<HTMLElement>('[data-map-card]')?.dataset.mapCard;
      if (mapCard) {
        void this.actions.openBoard(mapCard);
        return;
      }
      const mapPage = target.closest<HTMLElement>('[data-map-page]')?.dataset.mapPage;
      if (mapPage) {
        void this.openPageFromMap(mapPage);
        return;
      }

      const packVerb = target.closest<HTMLElement>('[data-pack-act]');
      if (packVerb) {
        const code = packVerb.closest<HTMLElement>('[data-pack]')?.dataset.pack;
        if (code) void this.usePack(code);
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

    // The first run forms. Submit rather than click, so Enter works and the
    // browser does not reload the page out from under a half finished setup.
    this.hosts.page.addEventListener('submit', (e) => {
      const form = e.target as HTMLElement | null;
      e.preventDefault();
      if (form?.id === 'setup-key') void this.saveKey();
      if (form?.id === 'setup-folder') void this.watchFolder();
      if (form?.id === 'pack-import') void this.importPack();
      if (form?.id === 'page-edit') void this.savePage();
      if (form?.id === 'notebook-ask') void this.askNotebook();
    });

    // Row selection, which is a change rather than a click.
    this.hosts.page.addEventListener('change', (e) => {
      const box = e.target as HTMLInputElement | null;
      if (!box?.classList.contains('pick')) return;
      const id = box.closest<HTMLElement>('[data-flag]')?.dataset.flag;
      if (!id) return;
      if (box.checked) this.state.picked.add(id);
      else this.state.picked.delete(id);
      this.state.confirmingDismiss = false;
      this.hosts.tools.innerHTML = bulkHTML(this.state.picked.size, this.state.confirmingDismiss);
    });
  }

  private async setupPack(code: string): Promise<void> {
    await this.setupStep(() => this.rpc.setPack(code).then(() => undefined));
  }

  private async saveKey(): Promise<void> {
    const input = this.hosts.body.querySelector<HTMLInputElement>('#setup-secret');
    const secret = input?.value.trim() ?? '';
    if (!secret) return;
    const keyRef = this.state.setup.run?.key_refs[0] ?? '';
    // Cleared before the call rather than after. The field holds the only copy
    // of the secret in this process, and a failed call leaves the screen up.
    if (input) input.value = '';
    await this.setupStep(async () => {
      await this.rpc.setKey(keyRef, secret);
      this.state.setup.keySaved = true;
    });
  }

  /**
   * Doc 16 section 3.4's verbs on a turn.
   *
   * Every one ends by re-reading the session, because what a turn shows is what
   * the core recorded about it: the grounding state is on the event log and the
   * page chip is on the card.
   */
  private async notebookVerb(verb: string, cardId?: string): Promise<void> {
    const boardId = this.state.notebook.session?.board_id;
    try {
      switch (verb) {
        case 'new': {
          const { board_id } = await this.rpc.openNotebook();
          this.state.notebook = {
            session: await this.rpc.notebookSession(board_id),
            asking: false,
          };
          break;
        }
        case 'save': {
          if (!boardId || !cardId) return;
          const saved = await this.rpc.saveAsPage(boardId, cardId);
          if (saved.created) this.actions.toast(`${COPY.savedAsPage}: ${saved.title ?? ''}`.trim());
          this.state.notebook = {
            session: await this.rpc.notebookSession(boardId),
            asking: false,
          };
          break;
        }
        case 'open-board': {
          if (!boardId || !cardId) return;
          const opened = await this.rpc.openOnBoard(boardId, cardId);
          await this.actions.openBoard(opened.board_id);
          return;
        }
        // Doc 16 section 3.4's one click way out of an ungrounded answer. The
        // first answer is superseded rather than replaced, so the session still
        // shows that the vault had nothing to say.
        case 'search-web': {
          if (!boardId || !cardId) return;
          this.actions.toast(COPY.notebookWebSearching);
          await this.rpc.searchWeb(boardId, cardId);
          this.state.notebook = {
            session: await this.rpc.notebookSession(boardId),
            asking: false,
          };
          break;
        }
        default:
          return;
      }
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
    }
  }

  private async askNotebook(): Promise<void> {
    const input = this.hosts.body.querySelector<HTMLInputElement>('#notebook-question');
    const question = input?.value.trim() ?? '';
    if (!question) return;
    if (input) input.value = '';

    // A session on the first question, so a person who types before pressing
    // the button gets what they asked for rather than a refusal.
    let boardId = this.state.notebook.session?.board_id;
    try {
      if (!boardId) boardId = (await this.rpc.openNotebook()).board_id;
      this.state.notebook = { ...this.state.notebook, asking: true };
      await this.render();
      await this.rpc.ask(boardId, question, 'deep');
      this.state.notebook = {
        session: await this.rpc.notebookSession(boardId),
        asking: false,
      };
    } catch (e) {
      this.state.notebook = { ...this.state.notebook, asking: false };
      this.actions.toast(e instanceof RpcError ? e.message : COPY.askFailed, 'error');
    }
    await this.render();
  }

  /**
   * Doc 16 section 3.7's verbs, in one place because they all end the same way:
   * the view redraws from what the core holds rather than from what this call
   * happened to return.
   */
  private async pageVerb(verb: string, pageId?: string, title?: string): Promise<void> {
    try {
      switch (verb) {
        case 'new':
          // An unsaved page rather than a written one: a person who changes
          // their mind should not leave an empty page behind.
          this.state.pages = {
            open: {
              id: '',
              title: '',
              body: '',
              file_path: '',
              updated_at: '',
              source_card_id: null,
              citations_carried: [],
              links: [],
              backlinks: [],
            },
            editing: true,
          };
          break;
        case 'open':
          if (!pageId) return;
          this.state.pages = { open: await this.rpc.page(pageId), editing: false };
          break;
        case 'close':
          this.state.pages = { open: null, editing: false };
          break;
        case 'edit':
          this.state.pages = { ...this.state.pages, editing: true };
          break;
        case 'preview':
          // Read what the core holds rather than what the textarea shows: the
          // preview is of the page, and the page is what was saved.
          if (this.state.pages.open?.id) {
            this.state.pages = {
              open: await this.rpc.page(this.state.pages.open.id),
              editing: false,
            };
          } else {
            this.state.pages = { ...this.state.pages, editing: false };
          }
          break;
        case 'remove': {
          if (!pageId) return;
          await this.rpc.deletePage(pageId);
          this.state.pages = { open: null, editing: false };
          break;
        }
        case 'follow-link': {
          if (!title) return;
          const { pages } = await this.rpc.pages();
          const found = pages.find((p) => p.title.toLowerCase() === title.toLowerCase());
          if (found) this.state.pages = { open: await this.rpc.page(found.id), editing: false };
          break;
        }
        case 'create-link': {
          // Doc 16 section 3.1: an unresolved link creates the page it names,
          // and lands the person in it.
          if (!title) return;
          const made = await this.rpc.createPageFromLink(title);
          this.state.pages = { open: await this.rpc.page(made.page_id), editing: true };
          break;
        }
        default:
          return;
      }
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
    }
  }

  private async savePage(): Promise<void> {
    const title = this.hosts.body.querySelector<HTMLInputElement>('#page-name')?.value ?? '';
    const body = this.hosts.body.querySelector<HTMLTextAreaElement>('#page-text')?.value ?? '';
    const open = this.state.pages.open;
    try {
      const saved = await this.rpc.writePage({
        page_id: open?.id || undefined,
        title,
        body,
      });
      this.state.pages = { open: await this.rpc.page(saved.page_id), editing: false };
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.pagesWriteFailed, 'error');
    }
  }

  /**
   * Doc 10 section 9. The path is read by the core, which validates the file
   * before anything else sees it, and the page redraws from `profile.get` so
   * what it shows is the library rather than what this call happened to return.
   */
  private async importPack(): Promise<void> {
    const path = this.hosts.body.querySelector<HTMLInputElement>('#pack-path')?.value ?? '';
    if (!path.trim()) return;
    try {
      const added = await this.rpc.importPack(path.trim());
      await this.render();
      this.actions.toast(`${COPY.profilePackImported} ${added.code}`);
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
    }
  }

  private async usePack(code: string): Promise<void> {
    try {
      await this.rpc.setPack(code);
      await this.render();
    } catch (e) {
      this.actions.toast(e instanceof RpcError ? e.message : COPY.pageUnread, 'error');
    }
  }

  private async watchFolder(): Promise<void> {
    const root = this.hosts.body.querySelector<HTMLInputElement>('#setup-folder-root')?.value ?? '';
    const label =
      this.hosts.body.querySelector<HTMLInputElement>('#setup-folder-label')?.value ?? '';
    const sensitive =
      this.hosts.body.querySelector<HTMLInputElement>('#setup-folder-sensitive')?.checked ?? false;
    if (!root.trim()) return;
    await this.setupStep(async () => {
      const added = await this.rpc.watchFolder({
        root: root.trim(),
        label: label.trim() || root.trim(),
        sensitive,
      });
      this.state.setup.folderAdded = {
        label: added.label,
        indexed: added.indexed,
        unreadable: added.errors.length,
      };
    });
  }

  /**
   * Run one setup step and redraw, keeping the error where the person is
   * looking rather than in a toast that has scrolled away by the time they read
   * the step it belongs to.
   */
  private async setupStep(work: () => Promise<void>): Promise<void> {
    this.state.setup.busy = true;
    this.state.setup.error = null;
    await this.render();
    try {
      await work();
    } catch (e) {
      this.state.setup.error = e instanceof RpcError ? e.message : COPY.pageUnread;
    }
    this.state.setup.busy = false;
    await this.render();
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
    const ids = [...this.state.picked];
    switch (action) {
      case 'accept':
        // Doc 09 section 6: bulk Accept needs no confirmation, because the flag
        // stands and the content stays hidden.
        await this.decide(ids, 'accept');
        return;
      case 'dismiss':
        // The first click asks; the second decides.
        this.state.confirmingDismiss = true;
        this.hosts.tools.innerHTML = bulkHTML(this.state.picked.size, true);
        return;
      case 'dismiss-confirm':
        await this.decide(ids, 'dismiss');
        return;
      case 'clear':
        this.state.picked.clear();
        this.state.confirmingDismiss = false;
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
