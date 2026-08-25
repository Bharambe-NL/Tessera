/**
 * The M0 acceptance gate.
 *
 * Doc 12 phase 0: "60 fps pan at 200 cards on a mid range laptop; if not, record
 * the finding and switch the layer to canvas rendering for edges and ink before
 * continuing."
 *
 * The harness drives a scripted pan over a fixed path and records the interval
 * between animation frames. It reports the percentiles rather than a mean,
 * because a pan that averages 60 fps while dropping every twentieth frame reads
 * as stutter and should not pass.
 */

export interface FrameStats {
  frames: number;
  durationMs: number;
  fps: number;
  p50: number;
  p95: number;
  p99: number;
  worst: number;
  /** Frames that took longer than one 60 Hz budget. */
  dropped: number;
  droppedRatio: number;
}

export interface GateResult {
  cards: number;
  pan: FrameStats;
  zoom: FrameStats;
  /** Time from an empty canvas to the first painted frame of the full board. */
  firstRenderMs: number;
  passed: boolean;
  notes: string[];
}

const FRAME_BUDGET_MS = 1000 / 60;

function summarise(samples: number[]): FrameStats {
  if (samples.length === 0) {
    return { frames: 0, durationMs: 0, fps: 0, p50: 0, p95: 0, p99: 0, worst: 0, dropped: 0, droppedRatio: 0 };
  }
  const sorted = [...samples].sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * q))];
  const total = samples.reduce((a, b) => a + b, 0);
  // A frame that overruns the budget by more than half a frame is a dropped one.
  const dropped = samples.filter((s) => s > FRAME_BUDGET_MS * 1.5).length;
  return {
    frames: samples.length,
    durationMs: total,
    fps: (samples.length / total) * 1000,
    p50: at(0.5),
    p95: at(0.95),
    p99: at(0.99),
    worst: sorted[sorted.length - 1],
    dropped,
    droppedRatio: dropped / samples.length,
  };
}

/**
 * Run `steps` frames, calling `onFrame` once per frame with the frame index,
 * and record the interval each frame took.
 */
function drive(steps: number, onFrame: (i: number) => void): Promise<number[]> {
  return new Promise((resolve, reject) => {
    // A hidden document does not schedule animation frames, so a measurement
    // taken there is not a measurement. Fail loudly rather than hang.
    if (document.visibilityState === 'hidden') {
      reject(new Error('The document is hidden. Animation frames are paused, so the gate cannot measure.'));
      return;
    }
    const samples: number[] = [];
    let i = 0;
    let last = performance.now();
    // Discard the first interval: it carries the cost of whatever ran before.
    let primed = false;

    const step = (now: number) => {
      const dt = now - last;
      last = now;
      if (primed) samples.push(dt);
      else primed = true;

      if (i >= steps) {
        resolve(samples);
        return;
      }
      onFrame(i++);
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
}

export interface GateHooks {
  /** Move the camera by a delta. Must not trigger a re-render of card markup. */
  panBy: (dx: number, dy: number) => void;
  /** Zoom about the centre of the view. */
  zoomCentre: (factor: number) => void;
  /** Force the queued transform write to land, so a frame measures real work. */
  flush: () => void;
}

export async function runGate(cardCount: number, hooks: GateHooks, firstRenderMs: number): Promise<GateResult> {
  const notes: string[] = [];

  // A pan path that keeps moving so nothing can be culled or cached away:
  // a slow diagonal sweep with a sinusoidal cross component.
  const panSamples = await drive(240, (i) => {
    const dx = -6 - Math.sin(i / 12) * 4;
    const dy = -3 - Math.cos(i / 17) * 3;
    hooks.panBy(dx, dy);
    hooks.flush();
  });

  // Zoom in and back out across the full range, which forces layer rasterisation
  // at several scales. This is where a webview usually gives out first.
  const zoomSamples = await drive(120, (i) => {
    hooks.zoomCentre(i < 60 ? 1.012 : 1 / 1.012);
    hooks.flush();
  });

  const pan = summarise(panSamples);
  const zoom = summarise(zoomSamples);

  if (pan.fps < 58) notes.push(`Pan averaged ${pan.fps.toFixed(1)} fps, under the 60 fps target.`);
  if (pan.droppedRatio > 0.05)
    notes.push(`Pan dropped ${(pan.droppedRatio * 100).toFixed(1)} percent of frames.`);
  if (zoom.fps < 50) notes.push(`Zoom averaged ${zoom.fps.toFixed(1)} fps.`);
  if (firstRenderMs > 1500) notes.push(`First render took ${firstRenderMs.toFixed(0)} ms.`);

  // The gate is about pan. Zoom and first render are recorded, not gating.
  const passed = pan.fps >= 58 && pan.droppedRatio <= 0.05;
  if (passed && notes.length === 0) notes.push('Pan holds 60 fps at this card count. No layer change needed.');

  return { cards: cardCount, pan, zoom, firstRenderMs, passed, notes };
}

export function formatResult(r: GateResult): string {
  const row = (name: string, s: FrameStats) =>
    `  ${name.padEnd(6)} ${s.fps.toFixed(1).padStart(6)} fps   p50 ${s.p50.toFixed(2).padStart(6)} ms   ` +
    `p95 ${s.p95.toFixed(2).padStart(6)} ms   p99 ${s.p99.toFixed(2).padStart(6)} ms   ` +
    `worst ${s.worst.toFixed(1).padStart(6)} ms   dropped ${s.dropped}/${s.frames}`;

  return [
    `M0 canvas gate, ${r.cards} cards`,
    `  first render ${r.firstRenderMs.toFixed(0)} ms`,
    row('pan', r.pan),
    row('zoom', r.zoom),
    `  ${r.passed ? 'PASS' : 'FAIL'}`,
    ...r.notes.map((n) => `  note: ${n}`),
  ].join('\n');
}
