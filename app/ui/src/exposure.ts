/**
 * Exposure. Doc 17 section 2.2 and phase 13g-ii.
 *
 * "A card that links the concept is read" moves a concept from unseen to
 * exposed, and the only thing that can tell whether a card was read is the
 * thing showing it. So the shell watches, and the core folds: `card.viewed.v1`
 * is the one writer of that transition, which is what keeps a replay able to
 * say why a concept is where it is.
 *
 * What counts as reading is a guess, and doc 17 open question 2 says so:
 * "three seconds of hover is a guess; measure on yourself first". It is here as
 * one named constant rather than a number spread through the handlers, so
 * measuring it later is an edit in one place.
 *
 * Dwell rather than appearance. A board that reported every card on screen
 * would mark a whole board read at a glance, and a map filled that way says the
 * learner has met twenty ideas when they have seen a wall. A card that stays
 * more than half in view for three seconds is a card somebody is looking at.
 */

/** Doc 17 open question 2's guess, named so it can be changed after measuring. */
export const EXPOSURE_MS = 3000;

/** How much of a card has to be in view for the clock to run. */
const VISIBLE = 0.5;

export interface ExposureHost {
  /** Where the cards are, so the observer has a root to watch inside. */
  cards: HTMLElement;
  /** Tell the core. Called at most once per card while this shell is open. */
  report: (cardId: string) => void;
}

/**
 * Watch the board and report a card the learner dwelt on.
 *
 * Once per card per shell, because exposure is capped anyway and a log that
 * carried a line every time a card scrolled past would be a log about
 * scrolling. Reading it again tomorrow is more exposure, and tomorrow is a new
 * shell.
 */
export function watchExposure(host: ExposureHost): () => void {
  const reported = new Set<string>();
  const timers = new Map<string, number>();

  const stop = (id: string): void => {
    const timer = timers.get(id);
    if (timer !== undefined) window.clearTimeout(timer);
    timers.delete(id);
  };

  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        const id = (entry.target as HTMLElement).dataset.cardId;
        if (!id || reported.has(id)) continue;
        if (!entry.isIntersecting) {
          stop(id);
          continue;
        }
        if (timers.has(id)) continue;
        timers.set(
          id,
          window.setTimeout(() => {
            timers.delete(id);
            if (reported.has(id)) return;
            reported.add(id);
            host.report(id);
          }, EXPOSURE_MS),
        );
      }
    },
    { threshold: VISIBLE },
  );

  // The board redraws whole on every change, so the observed set is rebuilt
  // rather than added to: watching a node that has been replaced is watching
  // nothing, and the timer it started would fire for a card nobody can see.
  const rewatch = (): void => {
    observer.disconnect();
    for (const id of [...timers.keys()]) stop(id);
    for (const card of host.cards.querySelectorAll<HTMLElement>('.card[data-card-id]')) {
      if (!reported.has(card.dataset.cardId ?? '')) observer.observe(card);
    }
  };
  rewatch();

  const mutations = new MutationObserver(rewatch);
  mutations.observe(host.cards, { childList: true });

  return () => {
    mutations.disconnect();
    observer.disconnect();
    for (const id of [...timers.keys()]) stop(id);
  };
}
