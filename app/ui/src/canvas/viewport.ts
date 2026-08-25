/**
 * Pan, zoom and view animation.
 *
 * Ported from the prototype (`canvas-prototype.html:726`-780). The port adds
 * one thing the prototype does not have and the 200 card gate needs: the world
 * transform is written inside a single `requestAnimationFrame` callback rather
 * than synchronously on every wheel or pointermove event, so a burst of input
 * costs one style recalculation per frame instead of one per event.
 */

import type { Viewport } from './types.js';

export const MIN_K = 0.2;
export const MAX_K = 2;

/** Doc 11 section 7: view animation is 420 ms quart out. */
const VIEW_MS = 420;
const quartOut = (t: number) => 1 - Math.pow(1 - t, 4);

export interface ViewportHostOptions {
  /** The element the pointer events land on and whose rect frames the view. */
  main: HTMLElement;
  /** The transformed layer holding cards, edges and ink. */
  world: HTMLElement;
  /** Called after every applied view change, for persisting the camera. */
  onSettled?: (view: Viewport) => void;
  /** Doc 11 section 7. When true, animations resolve instantly. */
  reducedMotion?: () => boolean;
}

export class ViewportHost {
  readonly view: Viewport = { x: 0, y: 0, k: 1 };

  private readonly main: HTMLElement;
  private readonly world: HTMLElement;
  private readonly onSettled?: (view: Viewport) => void;
  private readonly reducedMotion: () => boolean;

  private frame = 0;
  private animation = 0;

  constructor(opts: ViewportHostOptions) {
    this.main = opts.main;
    this.world = opts.world;
    this.onSettled = opts.onSettled;
    this.reducedMotion =
      opts.reducedMotion ??
      (() => window.matchMedia('(prefers-reduced-motion: reduce)').matches);
  }

  /**
   * Queue a transform write for the next frame. Calling this many times inside
   * one frame costs one write. This is the hot path under a pan.
   */
  apply(): void {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      const { x, y, k } = this.view;
      this.world.style.transform = `translate3d(${x}px, ${y}px, 0) scale(${k})`;
    });
  }

  /** Write the transform now, skipping the frame queue. For tests and teardown. */
  applySync(): void {
    if (this.frame) {
      cancelAnimationFrame(this.frame);
      this.frame = 0;
    }
    const { x, y, k } = this.view;
    this.world.style.transform = `translate3d(${x}px, ${y}px, 0) scale(${k})`;
  }

  panBy(dx: number, dy: number): void {
    this.cancelAnimation();
    this.view.x += dx;
    this.view.y += dy;
    this.apply();
  }

  /** Zoom about a client point, keeping that point fixed on the board. */
  zoomAt(clientX: number, clientY: number, factor: number): void {
    this.cancelAnimation();
    const rect = this.main.getBoundingClientRect();
    const mx = clientX - rect.left;
    const my = clientY - rect.top;
    const v = this.view;
    const k = Math.min(MAX_K, Math.max(MIN_K, v.k * factor));
    v.x = mx - (mx - v.x) * (k / v.k);
    v.y = my - (my - v.y) * (k / v.k);
    v.k = k;
    this.apply();
    this.onSettled?.(v);
  }

  zoomCentre(factor: number): void {
    const r = this.main.getBoundingClientRect();
    this.zoomAt(r.left + r.width / 2, r.top + r.height / 2, factor);
  }

  /** Board coordinates at the centre of the visible area, where a new card lands. */
  viewCentre(): { x: number; y: number } {
    const r = this.main.getBoundingClientRect();
    const v = this.view;
    return { x: (r.width / 2 - v.x) / v.k, y: (r.height * 0.42 - v.y) / v.k };
  }

  animateTo(tx: number, ty: number, tk: number): void {
    this.cancelAnimation();
    if (this.reducedMotion()) {
      this.view.x = tx;
      this.view.y = ty;
      this.view.k = tk;
      this.apply();
      this.onSettled?.(this.view);
      return;
    }
    const from = { ...this.view };
    const t0 = performance.now();
    const step = (now: number) => {
      const t = Math.min(1, (now - t0) / VIEW_MS);
      const e = quartOut(t);
      this.view.x = from.x + (tx - from.x) * e;
      this.view.y = from.y + (ty - from.y) * e;
      this.view.k = from.k + (tk - from.k) * e;
      this.applySync();
      if (t < 1) this.animation = requestAnimationFrame(step);
      else {
        this.animation = 0;
        this.onSettled?.(this.view);
      }
    };
    this.animation = requestAnimationFrame(step);
  }

  /** Fit a bounding box, in board coordinates, into the visible area. */
  fit(bounds: { x: number; y: number; w: number; h: number }): void {
    const rect = this.main.getBoundingClientRect();
    if (bounds.w <= 0 || bounds.h <= 0) return;
    const k = Math.min(1, (rect.width - 120) / bounds.w, (rect.height - 200) / bounds.h);
    const cx = bounds.x + bounds.w / 2;
    const cy = bounds.y + bounds.h / 2;
    this.animateTo(rect.width / 2 - cx * k, (rect.height - 120) / 2 - cy * k, k);
  }

  reset(): void {
    this.animateTo(0, 0, 1);
  }

  private cancelAnimation(): void {
    if (this.animation) {
      cancelAnimationFrame(this.animation);
      this.animation = 0;
    }
  }

  /** Wire wheel and drag panning. Returns a teardown function. */
  attach(): () => void {
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (e.ctrlKey || e.metaKey) this.zoomAt(e.clientX, e.clientY, Math.exp(-e.deltaY * 0.01));
      else this.panBy(-e.deltaX, -e.deltaY);
    };

    let dragging: { x: number; y: number } | null = null;
    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0) return;
      if ((e.target as HTMLElement).closest('[data-no-pan]')) return;
      dragging = { x: e.clientX, y: e.clientY };
      this.main.setPointerCapture(e.pointerId);
    };
    const onPointerMove = (e: PointerEvent) => {
      if (!dragging) return;
      this.panBy(e.clientX - dragging.x, e.clientY - dragging.y);
      dragging = { x: e.clientX, y: e.clientY };
    };
    const onPointerUp = (e: PointerEvent) => {
      if (!dragging) return;
      dragging = null;
      if (this.main.hasPointerCapture(e.pointerId)) this.main.releasePointerCapture(e.pointerId);
      this.onSettled?.(this.view);
    };

    this.main.addEventListener('wheel', onWheel, { passive: false });
    this.main.addEventListener('pointerdown', onPointerDown);
    this.main.addEventListener('pointermove', onPointerMove);
    this.main.addEventListener('pointerup', onPointerUp);
    this.main.addEventListener('pointercancel', onPointerUp);

    return () => {
      this.main.removeEventListener('wheel', onWheel);
      this.main.removeEventListener('pointerdown', onPointerDown);
      this.main.removeEventListener('pointermove', onPointerMove);
      this.main.removeEventListener('pointerup', onPointerUp);
      this.main.removeEventListener('pointercancel', onPointerUp);
      this.cancelAnimation();
      if (this.frame) cancelAnimationFrame(this.frame);
    };
  }
}
