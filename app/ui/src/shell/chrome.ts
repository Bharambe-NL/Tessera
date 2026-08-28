/**
 * The chrome around the canvas: composer, toolbar, title, rail toggle, reading
 * view, the pack update offer and the paste target.
 *
 * Doc 11 section 5 draws this frame; every listener here binds to an element
 * that exists for the life of the shell, so nothing needs rewiring when the
 * board redraws.
 */

import { boundsOf } from '../canvas/layout.js';
import type { Depth } from '../canvas/types.js';
import { RpcError } from '../rpc.js';
import { COPY } from '../strings.js';
import {
  ask,
  composer,
  el,
  emptyState,
  learnToggle,
  packUpdate,
  readingEl,
  readingToggle,
  titleInput,
  world,
} from './dom.js';
import { learnState, startLearning } from './learn.js';
import {
  heightOf,
  reload,
  renderBoard,
  renderReading,
  rpc,
  state,
  submit,
  toast,
  viewport,
  whileRunning,
} from './state.js';

/** Clear the composer and ask. Its own input, so its own reset. */
function submitFromComposer(): void {
  const question = ask.value;
  ask.value = '';
  ask.style.height = 'auto';
  // Doc 14 section 4: with Learn on, the composer names a topic rather than
  // asking a question, and the tutor interviews before anything is asked.
  if (learnToggle.getAttribute('aria-pressed') === 'true' && !learnState.learning) {
    void startLearning(question);
    return;
  }
  void submit(question);
}

export function wireComposer(): void {
  composer.addEventListener('submit', (e) => {
    e.preventDefault();
    submitFromComposer();
  });

  ask.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submitFromComposer();
    }
  });

  // Grow with the question rather than scrolling a one line box.
  ask.addEventListener('input', () => {
    ask.style.height = 'auto';
    ask.style.height = `${Math.min(ask.scrollHeight, 160)}px`;
  });

  for (const button of document.querySelectorAll<HTMLButtonElement>('#modes button')) {
    button.addEventListener('click', () => {
      state.depth = (button.dataset.depth as Depth) ?? 'fast';
      for (const b of document.querySelectorAll('#modes button')) b.classList.remove('on');
      button.classList.add('on');
    });
  }

  el<HTMLButtonElement>('zoom-in').addEventListener('click', () => viewport.zoomCentre(1.25));
  el<HTMLButtonElement>('zoom-out').addEventListener('click', () => viewport.zoomCentre(0.8));
  el<HTMLButtonElement>('fit').addEventListener('click', () => {
    if (state.board) viewport.fit(boundsOf(state.board.cards, heightOf));
  });
  // Tidy is the undo a drag has, so the release has to outlive the window the
  // same way the drag does. Only the cards that were pinned have anything to
  // write, and the write happens after the relayout so it carries the slot the
  // layout just chose rather than the place the card was dragged to.
  el<HTMLButtonElement>('tidy').addEventListener('click', () => {
    if (!state.board) return;
    const released = state.board.cards.filter((c) => c.position.pinned);
    for (const c of state.board.cards) {
      c.position.dx = 0;
      c.position.dy = 0;
      c.position.pinned = false;
    }
    renderBoard(state.board);

    const id = state.boardId;
    if (!id || released.length === 0) return;
    void (async () => {
      try {
        await Promise.all(released.map((c) => rpc.moveCard(id, c.id, c.position)));
      } catch {
        toast(COPY.moveFailed, 'error');
        await reload();
      }
    })();
  });
}

/**
 * Rename the board on blur or Enter.
 *
 * Doc 01 section 4.1: a board takes its title from the first question until a
 * person names it, and this is what stops that inference.
 */
export function wireTitle(): void {
  const commit = () => {
    const title = titleInput.value.trim();
    if (!state.boardId || !state.board || !title || title === state.board.title) return;
    const id = state.boardId;
    void rpc
      .rename(id, title)
      .then(() => {
        if (state.board) state.board.title = title;
      })
      .catch((e: unknown) => {
        toast(e instanceof RpcError ? e.message : COPY.renameFailed, 'error');
        if (state.board) titleInput.value = state.board.title;
      });
  };

  titleInput.addEventListener('blur', commit);
  titleInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      titleInput.blur();
    }
  });
}

/**
 * Swap the canvas for the same cards as a document.
 *
 * The canvas is hidden from assistive technology while the document is open and
 * the document is hidden while the canvas is, rather than both being present:
 * two copies of every card in the accessibility tree is worse than either one.
 */
export function wireReadingToggle(): void {
  readingToggle.addEventListener('click', () => {
    state.reading = !state.reading;
    readingToggle.setAttribute('aria-pressed', String(state.reading));
    readingToggle.textContent = state.reading ? COPY.readingClose : COPY.readingOpen;
    readingEl.hidden = !state.reading;
    world.setAttribute('aria-hidden', String(state.reading));
    emptyState.hidden = state.reading || (state.board?.cards.length ?? 0) > 0;
    if (state.reading && state.board) {
      renderReading(state.board);
      readingEl.focus();
    }
  });
}

export function wirePackUpdate(): void {
  packUpdate.addEventListener('click', () => {
    if (!state.boardId) return;
    const id = state.boardId;
    packUpdate.disabled = true;
    void rpc
      .updateBoardPack(id)
      .then(async (report) => {
        if (report.updated) {
          const flagged = report.cards.filter((c) => c.status === 'flagged').length;
          toast(
            `${COPY.boardPackUpdated} ${report.pack_code} ${report.to_version}: ` +
              `${report.cards.length} cards, ${flagged} flagged`,
          );
        }
        await reload();
      })
      .catch((e: unknown) => {
        toast(e instanceof RpcError ? e.message : COPY.boardPackUpdateFailed, 'error');
      })
      .finally(() => {
        packUpdate.disabled = false;
      });
  });
}

/** Doc 11 section 5: the rail is 56px collapsed and 240px open. */
export function wireRailToggle(): void {
  const rail = el<HTMLElement>('rail');
  const toggle = el<HTMLButtonElement>('rail-toggle');
  toggle.addEventListener('click', () => {
    // The class goes on `body` rather than on the rail, because the rail's
    // width, the board's left padding and the page's left edge all read one
    // custom property and it has to be set where all three inherit it.
    const open = document.body.classList.toggle('rail-open');
    rail.classList.toggle('open', open);
    toggle.setAttribute('aria-expanded', String(open));
  });
}

/**
 * Paste an image onto the board and read it. Doc 07 section A3.
 *
 * The paste is the whole gesture: doc 07 puts "Read this image" on an Image row
 * too, and that arrives with the Library's image surface. What a reader does
 * today is paste a screenshot of a table and want it read, so that is what this
 * does in one step.
 */
export function wirePaste(): void {
  document.addEventListener('paste', (e) => {
    if (!state.boardId || !rpc.connected) return;

    const items = [...(e.clipboardData?.items ?? [])];
    const file = items
      .find((item) => item.kind === 'file' && item.type.startsWith('image/'))
      ?.getAsFile();
    if (!file) return;

    // A paste that also carries text, into a box that takes text, is text. The
    // first version of this checked only the focus, which reads correctly and
    // is wrong in practice: `boot` focuses the composer, so the composer always
    // has focus and no image would ever have been read at all.
    const into = document.activeElement;
    const typing = into instanceof HTMLInputElement || into instanceof HTMLTextAreaElement;
    const alsoText = items.some((item) => item.kind === 'string' && item.type === 'text/plain');
    if (typing && alsoText) return;

    e.preventDefault();
    void readPastedImage(file);
  });
}

async function readPastedImage(file: File): Promise<void> {
  if (!state.boardId) return;
  const id = state.boardId;

  // The size is read from the image itself rather than trusted from the
  // clipboard, because the packet tells a vision model what it is looking at.
  const bitmap = await createImageBitmap(file).catch(() => null);
  if (!bitmap) {
    toast(COPY.readFailed, 'error');
    return;
  }

  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  const data = btoa(binary);

  await whileRunning(async () => {
    const { image_id } = await rpc.addImage(id, data, file.type, bitmap.width, bitmap.height);
    return await rpc.read(id, image_id);
  }, COPY.readFailed);
}
