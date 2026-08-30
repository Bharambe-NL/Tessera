/**
 * One read, one redraw, for whatever page is open.
 *
 * Every page follows the same shape: a title, a tool strip in the header, and a
 * body rendered from one read. The Router owns navigation and the verbs; this
 * module owns turning the state it is handed into what is on screen, and throws
 * rather than catches, because the Router is where a failed read is explained.
 */

import type { Rpc } from '../rpc.js';
import { COPY } from '../strings.js';
import { bulkHTML, flagsHTML } from './flags.js';
import { homeHTML, homeToolsHTML, type HomeFilter } from './home.js';
import { conceptsHTML, libraryToolsHTML, sourcesHTML, type LibraryTab } from './library.js';
import { mapHTML, mapToolsHTML, placementTiles, type MapState } from './map.js';
import { notebookHTML, notebookToolsHTML, type NotebookState } from './notebook.js';
import { pagesHTML, pagesToolsHTML, type PagesState } from './pages.js';
import { profileHTML } from './profile.js';
import type { View } from './router.js';
import { setupHTML, type SetupState } from './setup.js';

/**
 * State that survives a page switch, because a filter or a tab the user chose
 * should still be chosen when they come back to it. The Router holds it; the
 * render reads it and writes back what a read taught it.
 */
export interface PageState {
  homeFilter: HomeFilter;
  libraryTab: LibraryTab;
  pages: PagesState;
  notebook: NotebookState;
  map: MapState;
  /** Flags selected for a bulk decision, kept across a re-render. */
  picked: Set<string>;
  /** Doc 09 section 6: bulk Dismiss takes a second click with the count shown. */
  confirmingDismiss: boolean;
  /** What the first run screen is showing, on top of what the core reports. */
  setup: SetupState;
}

export interface PageHosts {
  title: HTMLElement;
  tools: HTMLElement;
  body: HTMLElement;
}

export async function renderPage(
  view: View,
  hosts: PageHosts,
  rpc: Rpc,
  s: PageState,
  setFlagCount: (n: number) => void,
): Promise<void> {
  const { title, tools, body } = hosts;
  switch (view) {
    case 'home': {
      title.textContent = COPY.railHome;
      tools.innerHTML = homeToolsHTML(s.homeFilter);
      const { boards } = await rpc.listBoards(s.homeFilter);
      // Doc 17 section 6's last line. A profile with no mission has no
      // summary rather than an empty one, and a read that failed is not
      // worth taking Home down for.
      const mission = await rpc.missionSummary().catch(() => null);
      body.innerHTML = homeHTML(boards, s.homeFilter, mission);
      break;
    }
    case 'flags': {
      title.textContent = COPY.railFlags;
      const { flags } = await rpc.flags();
      // A flag decided elsewhere should not stay selected here.
      const live = new Set(flags.map((f) => f.id));
      for (const id of [...s.picked]) if (!live.has(id)) s.picked.delete(id);
      tools.innerHTML = bulkHTML(s.picked.size, s.confirmingDismiss);
      body.innerHTML = flagsHTML(flags, s.picked);
      setFlagCount(flags.length);
      break;
    }
    case 'library': {
      title.textContent = COPY.railLibrary;
      tools.innerHTML = libraryToolsHTML(s.libraryTab);
      if (s.libraryTab === 'sources') {
        const { sources } = await rpc.sources();
        body.innerHTML = sourcesHTML(sources);
      } else {
        const { concepts } = await rpc.concepts();
        body.innerHTML = conceptsHTML(concepts);
      }
      break;
    }
    case 'notebook': {
      // Doc 16 section 3.4. One session at a time, the most recent unless
      // the person started another, because a chat with three histories on
      // screen is not a chat.
      title.textContent = COPY.railNotebook;
      if (!s.notebook.session) {
        const { sessions } = await rpc.notebookSessions();
        if (sessions.length > 0) {
          s.notebook = {
            session: await rpc.notebookSession(sessions[0].id),
            asking: false,
          };
        }
      }
      tools.innerHTML = notebookToolsHTML(s.notebook);
      body.innerHTML = notebookHTML(s.notebook);
      break;
    }
    case 'pages': {
      // Doc 16 section 3.7. One view with two states: the explorer, and one
      // page open. The list is re-read on every render rather than kept,
      // because a page saved from a card arrives without this view knowing.
      title.textContent = COPY.railPages;
      tools.innerHTML = pagesToolsHTML(s.pages);
      const { pages } = await rpc.pages();
      body.innerHTML = pagesHTML(pages, s.pages);
      break;
    }
    case 'map': {
      // Doc 17 section 6. The read is whole every time: a rating, a check
      // or a lesson elsewhere changes what a node is, and a map that kept
      // its last answer would show a state the learner had already left.
      title.textContent = COPY.railMap;
      s.map.map = await rpc.readMap();
      // Doc 17 section 3: placement opens itself the first time there is
      // something to rate, and never again once the learner has left it.
      // Null means nobody has decided yet, which is only true of the first
      // read: a learner who went to the map has decided.
      if (s.map.placing === null) s.map.placing = placementTiles(s.map).length > 0;
      if (s.map.open) {
        const fresh = s.map.map.concepts.find((c) => c.concept_id === s.map.open?.concept_id);
        s.map.open = fresh ?? null;
        if (!s.map.open) s.map.links = null;
      }
      tools.innerHTML = mapToolsHTML(s.map);
      body.innerHTML = mapHTML(s.map);
      break;
    }
    case 'profile': {
      // One page, five sections, no tabs. Owner decision 2026-08-30.
      title.textContent = COPY.railProfile;
      tools.innerHTML = '';
      body.innerHTML = profileHTML(await rpc.profile());
      break;
    }
    case 'setup': {
      title.textContent = COPY.setupTitle;
      tools.innerHTML = '';
      // Re-read on every render, so a key added in one step shows as done
      // in the next without this screen keeping its own idea of the truth.
      s.setup.run = await rpc.firstRun();
      body.innerHTML = setupHTML(s.setup);
      break;
    }
  }
}
