/**
 * What a branch hangs from.
 *
 * Doc 09 section 5's Branch verb has two forms on a card: "Highlight or block
 * branch". A highlight carries the selected span, a block carries the JSON
 * pointer `BlockIndexEntry.ref` that the Visualizer already writes into
 * `data-ref` on every clickable block. Both produce a card of kind `branch`;
 * the core decides that from which anchor is present.
 *
 * The rect is in client coordinates. The popover lives outside `#world`, so it
 * holds its size when the board is zoomed, which means no board coordinate
 * conversion belongs here: converting and then converting back would move the
 * popover with the camera and scale its text with it.
 */

/** The longest span worth carrying as a card title. Doc 01 section 4.4. */
const MAX_ANCHOR = 240;

export interface AnchorTarget {
  cardId: string;
  /** Exactly one of these two is set. */
  anchorText?: string;
  anchorBlockRef?: string;
  /** What the popover shows above the question box, so the user sees the subject. */
  label: string;
  /** Where to put the popover, in client coordinates. */
  rect: DOMRect;
}

function cardIdOf(node: Node | null): string | null {
  const el = node instanceof Element ? node : node?.parentElement;
  return el?.closest<HTMLElement>('.card')?.dataset.cardId ?? null;
}

/**
 * Read the current selection, if it is a span inside one card's body.
 *
 * A selection spanning two cards is refused rather than truncated: it names no
 * single card, and a branch has exactly one parent.
 */
export function selectionAnchor(): AnchorTarget | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;

  const text = selection.toString().trim();
  if (!text) return null;

  const range = selection.getRangeAt(0);
  const cardId = cardIdOf(range.startContainer);
  if (!cardId || cardId !== cardIdOf(range.endContainer)) return null;

  // Only the body. A selection in the header or the follow-up box is a person
  // reading or editing, not one marking a claim.
  const start = range.startContainer instanceof Element ? range.startContainer : range.startContainer.parentElement;
  if (!start?.closest('.card .body')) return null;

  const rect = range.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) return null;

  return {
    cardId,
    anchorText: text.slice(0, MAX_ANCHOR),
    label: text.length > 80 ? `${text.slice(0, 80)}…` : text,
    rect,
  };
}

/**
 * Read a click on a visual's block.
 *
 * `data-ref` is the JSON pointer into the visual's payload, written by
 * `visual.ts` for exactly this: an exact reference rather than a label match.
 */
export function blockAnchor(target: EventTarget | null): AnchorTarget | null {
  if (!(target instanceof Element)) return null;
  const block = target.closest<HTMLElement>('[data-ref]');
  const ref = block?.dataset.ref;
  if (!block || !ref) return null;

  // A hidden block is a placeholder carrying a flag reason. There is nothing
  // behind it to investigate until the flag is decided.
  if (block.classList.contains('block-hidden')) return null;

  const cardId = cardIdOf(block);
  if (!cardId) return null;

  const label = (block.textContent ?? '').trim().replace(/\s+/g, ' ');
  return {
    cardId,
    anchorBlockRef: ref,
    label: label.length > 80 ? `${label.slice(0, 80)}…` : label || ref,
    rect: block.getBoundingClientRect(),
  };
}
