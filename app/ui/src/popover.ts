/**
 * The anchor popover, in its two states.
 *
 * Doc 09 section 3 lists a highlight popover and a block investigate popover on
 * the board surface. They ask the same thing of the user, so this is one
 * element following the prototype's two step interaction
 * (`Docs/canvas-prototype.html:331`-336): offer first, so a selection made while
 * reading is not interrupted by a text box, then compose once the offer is
 * taken.
 */

import type { AnchorTarget } from './canvas/anchor.js';
import { COPY } from './strings.js';

/** Kept clear of the viewport edge, so the popover never opens half off screen. */
const MARGIN = 12;

export interface PopoverHosts {
  root: HTMLElement;
  label: HTMLElement;
  ask: HTMLButtonElement;
  /** Doc 16 section 3.6's "Add note", offered beside the branch. */
  note: HTMLButtonElement;
  compose: HTMLFormElement;
  question: HTMLInputElement;
  cancel: HTMLButtonElement;
}

export class AnchorPopover {
  private target: AnchorTarget | null = null;

  constructor(
    private readonly hosts: PopoverHosts,
    /** Called with the anchor and the question once the user commits. */
    private readonly onBranch: (target: AnchorTarget, question: string) => void,
    /** Called with the anchor when the user keeps the quote as a sticky. */
    private readonly onSticky: (target: AnchorTarget) => void,
  ) {}

  get open(): boolean {
    return !this.hosts.root.hidden;
  }

  /** The anchor currently offered, for a caller deciding whether to reopen. */
  get anchored(): AnchorTarget | null {
    return this.target;
  }

  show(target: AnchorTarget): void {
    const { root, label, ask, compose, question } = this.hosts;
    this.target = target;

    label.textContent = target.label;
    ask.textContent = target.anchorBlockRef ? COPY.investigateBlock : COPY.askAboutThis;
    // A sticky quotes what was selected, so a block's pointer has nothing to
    // prefill it with. Doc 16 section 3.6 offers it "from the highlight menu".
    this.hosts.note.hidden = !target.anchorText;
    this.hosts.note.textContent = COPY.addSticky;
    compose.hidden = true;
    question.value = '';
    root.hidden = false;

    this.place(target.rect);
  }

  /** Move from the offer to the question box. */
  private compose(): void {
    this.hosts.compose.hidden = false;
    this.hosts.question.focus();
    if (this.target) this.place(this.target.rect);
  }

  close(): void {
    this.hosts.root.hidden = true;
    this.hosts.compose.hidden = true;
    this.target = null;
  }

  private place(rect: DOMRect): void {
    const root = this.hosts.root;
    // Measure after the content is in, because the compose state is taller than
    // the offer state and a popover placed at the offer height overlaps the
    // selection once the box appears.
    const box = root.getBoundingClientRect();
    const left = Math.min(
      Math.max(MARGIN, rect.left + rect.width / 2 - box.width / 2),
      window.innerWidth - box.width - MARGIN,
    );
    // Above the anchor when there is room, below it when there is not.
    const above = rect.top - box.height - 10;
    const top = above >= MARGIN ? above : Math.min(rect.bottom + 10, window.innerHeight - box.height - MARGIN);
    root.style.left = `${Math.round(left)}px`;
    root.style.top = `${Math.round(top)}px`;
  }

  /** Wire the popover's own controls. Returns a teardown function. */
  attach(): () => void {
    const { root, ask, note, compose, question, cancel } = this.hosts;

    const onAsk = () => this.compose();
    const onSticky = () => {
      if (!this.target) return;
      const target = this.target;
      this.close();
      this.onSticky(target);
    };
    const onCancel = () => this.close();
    const onSubmit = (e: Event) => {
      e.preventDefault();
      const text = question.value.trim();
      if (!text || !this.target) return;
      const target = this.target;
      this.close();
      this.onBranch(target, text);
    };
    // Escape closes from anywhere inside, which is the one keyboard affordance
    // a dialog needs before step 4's full pass.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        this.close();
      }
    };

    ask.addEventListener('click', onAsk);
    note.addEventListener('click', onSticky);
    cancel.addEventListener('click', onCancel);
    compose.addEventListener('submit', onSubmit);
    root.addEventListener('keydown', onKey);

    return () => {
      ask.removeEventListener('click', onAsk);
      note.removeEventListener('click', onSticky);
      cancel.removeEventListener('click', onCancel);
      compose.removeEventListener('submit', onSubmit);
      root.removeEventListener('keydown', onKey);
    };
  }
}
