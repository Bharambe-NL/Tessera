/**
 * The shell's element references, resolved once.
 *
 * Every id here is declared in index.html, which is served with the bundle, so
 * a missing element is a build defect rather than a runtime state. The cast in
 * `el` carries that assumption; nothing checks for null because there is no
 * good answer to a shell whose skeleton is absent.
 */

export const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

export const main = el<HTMLElement>('main');
export const world = el<HTMLElement>('world');
export const cardsEl = el<HTMLElement>('cards');
export const stickiesEl = el<HTMLElement>('stickies');
export const handlesEl = el<HTMLElement>('handles');
export const edgesEl = document.getElementById('edges') as unknown as SVGElement;
export const gateEl = el<HTMLPreElement>('gate');
export const composer = el<HTMLFormElement>('composer');
export const ask = el<HTMLTextAreaElement>('ask');
export const titleInput = el<HTMLInputElement>('title');
export const modeLabel = el<HTMLElement>('mode-label');
export const modeChip = el<HTMLElement>('mode');
export const emptyState = el<HTMLElement>('empty');
export const toasts = el<HTMLElement>('toasts');
export const readingEl = el<HTMLElement>('reading');
export const readingToggle = el<HTMLButtonElement>('reading-toggle');
export const packUpdate = el<HTMLButtonElement>('pack-update');
export const exerciseEl = el<HTMLElement>('exercise');
export const exerciseBody = el<HTMLElement>('ex-body');
export const tutorEl = el<HTMLElement>('tutor');
export const tutorBody = el<HTMLElement>('tutor-body');
export const tutorStage = el<HTMLElement>('tutor-stage');
export const learnToggle = el<HTMLButtonElement>('learn');
