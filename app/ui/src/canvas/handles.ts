/**
 * Edge handles. Doc 16 section 3.6.
 *
 * "Four `+` handles on hover; drag out creates an empty follow-up card with the
 * composer focused (the prototype's card footer input does the same without the
 * drag; the handle adds discoverability)."
 *
 * One element for the whole board rather than four buttons per card. Doc 12
 * phase 0's gate is 60 fps pan at 200 cards, and `render.ts` earns it by
 * rebuilding a card's markup only when its signature changes: putting handles
 * inside that markup would add eight hundred elements to the card layer and put
 * a hover state into the signature, so every hover would rebuild a card. The
 * handles live outside it, are positioned by transform like everything else on
 * the canvas, and the card markup is untouched.
 *
 * An empty card is the one thing this cannot make: a Card carries a question
 * and the store requires it, so a handle that made one would leave a row no
 * pipeline could run. What the handle does is what the prototype's footer input
 * does, which is what doc 16 says it is for: it puts the cursor in the
 * follow-up box on that card.
 *
 * The handles are out of the tab order and hidden from assistive technology on
 * purpose. Doc 09 section 14 asks that every verb be reachable by keyboard, and
 * this one is: the follow-up box is in the tab order on every card and is the
 * thing the handle points at. A second tab stop for a shortcut to a control
 * already there would lengthen the walk through the board for the readers who
 * can least afford it.
 */

export const SIDES = ['top', 'right', 'bottom', 'left'] as const;
export type Side = (typeof SIDES)[number];

export interface HandleHosts {
  /** The card layer, for the hover delegation. */
  cards: HTMLElement;
  /** The handle overlay, a sibling of the card layer inside the world. */
  handles: HTMLElement;
}

/**
 * Follow the pointer from card to card, and report a handle that was used.
 *
 * Returns a teardown function, the way the other canvas hosts do.
 */
export function attachHandles(hosts: HandleHosts, onPull: (cardId: string, side: Side) => void): () => void {
  const { cards, handles } = hosts;

  const place = (card: HTMLElement) => {
    const id = card.dataset.cardId;
    if (!id) return;
    handles.dataset.cardId = id;
    // The card's own transform, read rather than recomputed: the layout wrote
    // it and reading it keeps the handles on the card through a tidy without
    // this file knowing anything about layout.
    handles.style.transform = card.style.transform;
    handles.style.width = `${card.offsetWidth}px`;
    handles.style.height = `${card.offsetHeight}px`;
    handles.hidden = false;
  };

  const onOver = (e: PointerEvent) => {
    const card = (e.target as HTMLElement | null)?.closest<HTMLElement>('.card');
    if (card) place(card);
  };

  // Leaving the card layer altogether. A pointer moving from the card onto its
  // own handles is still on the card as far as a person is concerned, and the
  // overlay sits above it, so the handles keep themselves.
  const onOut = (e: PointerEvent) => {
    const to = e.relatedTarget as HTMLElement | null;
    if (to?.closest('.card') || to?.closest('.handles')) return;
    handles.hidden = true;
  };

  const onClick = (e: Event) => {
    const button = (e.target as HTMLElement | null)?.closest<HTMLElement>('[data-side]');
    const side = button?.dataset.side as Side | undefined;
    const cardId = handles.dataset.cardId;
    if (!side || !cardId) return;
    handles.hidden = true;
    onPull(cardId, side);
  };

  cards.addEventListener('pointerover', onOver);
  cards.addEventListener('pointerout', onOut);
  handles.addEventListener('pointerout', onOut);
  handles.addEventListener('click', onClick);

  return () => {
    cards.removeEventListener('pointerover', onOver);
    cards.removeEventListener('pointerout', onOut);
    handles.removeEventListener('pointerout', onOut);
    handles.removeEventListener('click', onClick);
  };
}
