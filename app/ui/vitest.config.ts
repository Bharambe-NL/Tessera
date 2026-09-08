import { defineConfig } from 'vitest/config';

/**
 * The unit runner, for the pure functions playwright cannot reach.
 *
 * Playwright drives the built app against a real core, which is the right test
 * for a verb and the wrong one for arithmetic: to check that a dropped frame is
 * counted correctly you would have to make a browser drop a frame on cue. These
 * run the function directly instead.
 *
 * `happy-dom` rather than `node` because a module under `src` may touch
 * `document` at import time even when the function under test does not, and a
 * unit test failing on that would be a fact about the environment.
 */
export default defineConfig({
  test: {
    environment: 'happy-dom',
    include: ['src/**/*.test.ts'],
  },
});
