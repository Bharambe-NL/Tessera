/**
 * The gate decides whether the canvas layer changes, so its arithmetic has to
 * be right on a sequence somebody can check by hand. A browser cannot be asked
 * to drop a frame on cue, which is why this feeds `summarise` a fixed array
 * instead of measuring anything.
 */

import { describe, expect, it } from 'vitest';

import { formatResult, summarise, type GateResult } from './gate.js';

/** Nine frames inside a 60 Hz budget and one long one. */
const SAMPLES = [10, 10, 10, 10, 10, 10, 10, 10, 10, 40];

function result(passed: boolean): GateResult {
  return {
    cards: 200,
    pan: summarise(SAMPLES),
    zoom: summarise(SAMPLES),
    firstRenderMs: 420,
    passed,
    notes: [],
  };
}

describe('summarise', () => {
  it('reads nothing out of no samples', () => {
    const s = summarise([]);
    expect(s.frames).toBe(0);
    expect(s.fps).toBe(0);
    expect(s.dropped).toBe(0);
    expect(s.droppedRatio).toBe(0);
  });

  it('turns frame intervals into a rate', () => {
    const s = summarise(SAMPLES);
    // Ten frames across 130 ms of wall clock is 76.92 frames a second.
    expect(s.frames).toBe(10);
    expect(s.durationMs).toBe(130);
    expect(s.fps).toBeCloseTo((10 / 130) * 1000, 6);
  });

  it('counts a frame as dropped only past half a budget over', () => {
    const s = summarise(SAMPLES);
    // The budget is 16.67 ms and the threshold is one and a half of it, so the
    // 40 ms frame is dropped and the 10 ms ones are not.
    expect(s.dropped).toBe(1);
    expect(s.droppedRatio).toBeCloseTo(0.1, 6);
    expect(s.worst).toBe(40);

    // A run that never overruns drops nothing, whatever its length.
    expect(summarise([16, 16, 16, 16]).dropped).toBe(0);
    // One that always overruns drops everything.
    expect(summarise([30, 30, 30]).droppedRatio).toBe(1);
  });

  it('reports percentiles rather than a mean', () => {
    const s = summarise(SAMPLES);
    expect(s.p50).toBe(10);
    expect(s.p95).toBe(40);
    expect(s.p99).toBe(40);
  });
});

describe('formatResult', () => {
  it('says the verdict', () => {
    expect(formatResult(result(true))).toContain('PASS');
    expect(formatResult(result(false))).toContain('FAIL');
  });

  it('names the card count and both axes', () => {
    const text = formatResult(result(true));
    expect(text).toContain('200 cards');
    expect(text).toContain('pan');
    expect(text).toContain('zoom');
    expect(text).toContain('dropped 1/10');
  });
});
