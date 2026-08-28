/**
 * Every gesture the canvas takes: card verbs, drag, branching, handles,
 * stickies and the build trail.
 *
 * One listener for every verb on every card. Doc 09 section 5. Delegation
 * rather than a handler per card because `renderCards` rebuilds a card's markup
 * whenever its signature changes, and a listener bound to an element that gets
 * replaced stops firing without saying so. Everything here binds to containers,
 * which are never replaced.
 */

import { blockAnchor, selectionAnchor } from '../canvas/anchor.js';
import { trailFor, trailHTML } from '../canvas/built.js';
import { attachHandles } from '../canvas/handles.js';
import { drawEdges, toggleFlags } from '../canvas/render.js';
import type { Card } from '../canvas/types.js';
import { RpcError } from '../rpc.js';
import { COPY } from '../strings.js';
import { cardsEl, edgesEl, handlesEl, stickiesEl } from './dom.js';
import {
  heightOf,
  linked,
  reload,
  renderBoard,
  rpc,
  state,
  submit,
  toast,
  viewport,
  whileRunning,
} from './state.js';

/**
 * Fill one card's "How this was built" body from `board.history`.
 *
 * Read on open rather than on render: the disclosure is closed on most cards
 * most of the time, and the history is the whole board's log, so fetching it per
 * card per render would read the same hundreds of events once per card.
 */
async function fillBuildTrail(body: HTMLElement, cardId: string): Promise<void> {
  if (!state.boardId || body.dataset.filled === cardId) return;
  try {
    const { events } = await rpc.history(state.boardId);
    body.innerHTML = trailHTML(trailFor(cardId, events));
    body.dataset.filled = cardId;
  } catch {
    body.textContent = COPY.builtFailed;
  }
}

export function wireBuildTrail(): void {
  // `toggle` does not bubble, so it is caught in the capture phase. One listener
  // for every disclosure on the board, present and future.
  cardsEl.addEventListener(
    'toggle',
    (e) => {
      const details = e.target as HTMLDetailsElement | null;
      if (!details?.open || !details.classList.contains('built')) return;
      const body = details.querySelector<HTMLElement>('.built-body');
      const cardId = body?.dataset.builtFor;
      if (body && cardId) void fillBuildTrail(body, cardId);
    },
    true,
  );
}

/**
 * Offer to branch from a selected span or a clicked block.
 *
 * `pointerup` rather than `selectionchange`, because a selection being dragged
 * changes on every frame and a popover that follows it is unusable.
 */
export function wireBranching(): void {
  cardsEl.addEventListener('pointerup', () => {
    // After the browser has settled the selection this gesture produced.
    window.setTimeout(() => {
      const anchor = selectionAnchor();
      if (anchor) linked.popover.show(anchor);
      else if (linked.popover.anchored?.anchorText) linked.popover.close();
    }, 0);
  });

  cardsEl.addEventListener('click', (e) => {
    const anchor = blockAnchor(e.target);
    if (!anchor) return;
    e.stopPropagation();
    linked.popover.show(anchor);
  });

  // A click anywhere else puts the popover away, the way a menu behaves.
  document.addEventListener('pointerdown', (e) => {
    if (!linked.popover.open) return;
    const target = e.target as HTMLElement | null;
    if (target?.closest('#anchor-pop')) return;
    if (target?.closest('.card .body')) return;
    linked.popover.close();
  });
}

/**
 * Doc 16 section 3.6's edge handles, which put the cursor where the follow-up
 * is typed.
 *
 * A Card carries a question and the store requires one, so there is no empty
 * card for a handle to make. The prototype's footer input is what the handle
 * points at, which is what doc 16 says the handle is for.
 */
export function wireHandles(): void {
  attachHandles({ cards: cardsEl, handles: handlesEl }, (cardId) => {
    const input = document
      .getElementById(`card-${cardId}`)
      ?.querySelector<HTMLInputElement>('.followup');
    if (!input || input.disabled) return;
    input.focus();
  });
}

/** The one verb a sticky has: taking it off again. Doc 09 section 5's undo. */
export function wireStickies(): void {
  stickiesEl.addEventListener('click', (e) => {
    const target = e.target as HTMLElement | null;
    const noteId = target?.closest<HTMLElement>('[data-act="unstick"]')
      ? target.closest<HTMLElement>('.sticky')?.dataset.noteId
      : undefined;
    if (!noteId) return;
    void (async () => {
      try {
        await rpc.removeNote(noteId);
        await reload();
      } catch {
        toast(COPY.stickyFailed, 'error');
      }
    })();
  });
}

/**
 * Move a card by its head, and keep it where it was dropped.
 *
 * Doc 01 section 4.2 gives `position` a user offset and a `pinned` flag, and
 * the layout has honoured both since M0. Nothing ever set them: the canvas had
 * one drag and it panned the whole world, so every card moved together and a
 * board could not be arranged at all.
 *
 * The head is the handle rather than the whole card. Text in the body stays
 * selectable, which is what highlight to branch is built on, and a drag that
 * starts anywhere else still pans, so the gesture the board already had is
 * unchanged.
 */
export function wireCardDrag(): void {
  interface Drag {
    card: Card;
    el: HTMLElement;
    pointerId: number;
    fromX: number;
    fromY: number;
    atX: number;
    atY: number;
    moved: boolean;
  }
  let drag: Drag | null = null;
  let frame = 0;

  // Edges are redrawn on a frame rather than on every pointer event, because a
  // pointer fires faster than the screen and the edge layer is one SVG for the
  // whole board.
  const redrawEdges = () => {
    if (frame || !state.board) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (state.board) drawEdges(state.board.cards, edgesEl, heightOf, state.board.notes ?? []);
    });
  };

  cardsEl.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement | null;
    if (!target?.closest('.head')) return;
    // The head carries verbs of its own, and a press on one is not a drag.
    if (target.closest('button, a, input, [data-act]')) return;
    const el = target.closest<HTMLElement>('.card');
    const card = state.board?.cards.find((c) => c.id === el?.dataset.cardId);
    if (!el || !card) return;

    // The viewport pans from `main`, and this gesture is not a pan.
    e.stopPropagation();
    e.preventDefault();
    drag = {
      card,
      el,
      pointerId: e.pointerId,
      fromX: e.clientX,
      fromY: e.clientY,
      atX: card.position.x,
      atY: card.position.y,
      moved: false,
    };
    // Capture keeps the drag alive when the pointer leaves the card, and a
    // browser that refuses it still drags: the card simply stops following if
    // the pointer runs off it. A throw here would strand `drag` set with no way
    // to clear it, so the drag is worth more than the capture.
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      /* the drag works without it */
    }
    el.classList.add('dragging');
  });

  cardsEl.addEventListener('pointermove', (e) => {
    if (!drag || e.pointerId !== drag.pointerId) return;
    // A screen pixel is a world pixel divided by the zoom.
    const k = viewport.view.k || 1;
    const dx = (e.clientX - drag.fromX) / k;
    const dy = (e.clientY - drag.fromY) / k;
    // A press that never travels is a click on the head, not a move.
    if (!drag.moved && Math.abs(dx) < 3 && Math.abs(dy) < 3) return;
    drag.moved = true;
    drag.card.position.x = drag.atX + dx;
    drag.card.position.y = drag.atY + dy;
    // The transform is written straight, the way `renderCards` writes it, so
    // the card tracks the pointer without a relayout on every frame.
    drag.el.style.transform = `translate3d(${drag.card.position.x}px, ${drag.card.position.y}px, 0)`;
    redrawEdges();
  });

  const drop = (e: PointerEvent) => {
    if (!drag || e.pointerId !== drag.pointerId) return;
    const d = drag;
    drag = null;
    d.el.classList.remove('dragging');
    try {
      if (d.el.hasPointerCapture(e.pointerId)) d.el.releasePointerCapture(e.pointerId);
    } catch {
      /* it was never captured */
    }
    if (!d.moved) return;

    // `dx` is the offset from the slot the layout would have given this card,
    // so it accumulates the move rather than replacing it.
    d.card.position.dx += d.card.position.x - d.atX;
    d.card.position.dy += d.card.position.y - d.atY;
    d.card.position.pinned = true;
    if (state.board) renderBoard(state.board);

    const id = state.boardId;
    if (!id) return;
    void (async () => {
      try {
        await rpc.moveCard(id, d.card.id, d.card.position);
      } catch {
        // The board on screen disagrees with the board in the core, and the
        // core is the one that survives a reload.
        toast(COPY.moveFailed, 'error');
        await reload();
      }
    })();
  };

  cardsEl.addEventListener('pointerup', drop);
  cardsEl.addEventListener('pointercancel', drop);
}

export function wireCardActions(): void {
  cardsEl.addEventListener('click', (e) => {
    const target = e.target as HTMLElement | null;
    const button = target?.closest<HTMLElement>('[data-act]');
    const cardEl = target?.closest<HTMLElement>('.card');
    const cardId = cardEl?.dataset.cardId;
    if (!button || !cardId || !state.boardId) return;

    switch (button.dataset.act) {
      case 'flags': {
        toggleFlags(cardId);
        if (state.board) renderBoard(state.board);
        break;
      }
      case 'follow': {
        const input = cardEl?.querySelector<HTMLInputElement>('.followup');
        const question = input?.value ?? '';
        if (!question.trim()) {
          input?.focus();
          break;
        }
        if (input) input.value = '';
        void submit(question, { parentCardId: cardId });
        break;
      }
      case 'rerun': {
        const id = state.boardId;
        void whileRunning(() => rpc.verify(id, cardId), COPY.rerunFailed);
        break;
      }
      case 'save': {
        const id = state.boardId;
        void rpc
          .saveAsPage(id, cardId)
          .then(async (saved) => {
            if (saved.created) toast(`${COPY.savedAsPage}: ${saved.title ?? ''}`.trim());
            await reload();
          })
          .catch((e: unknown) => {
            toast(e instanceof RpcError ? e.message : COPY.saveFailed, 'error');
          });
        break;
      }
    }
  });

  // Enter in a card's follow-up box sends it, matching the composer.
  cardsEl.addEventListener('keydown', (e) => {
    const input = (e.target as HTMLElement | null)?.closest<HTMLInputElement>('.followup');
    if (!input || e.key !== 'Enter') return;
    e.preventDefault();
    const cardId = input.closest<HTMLElement>('.card')?.dataset.cardId;
    if (!cardId || !input.value.trim()) return;
    const question = input.value;
    input.value = '';
    void submit(question, { parentCardId: cardId });
  });
}
