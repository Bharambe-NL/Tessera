/**
 * Drive the built UI against a real core.
 *
 * `webServer` runs `tessera-ui-server`, which serves `dist` and one `POST /rpc`
 * endpoint over the same router the Tauri shell registers. That is what makes
 * these tests claims about the product rather than about a fixture: the RPC
 * calls go to a core, and a verb that is registered but unreachable fails here.
 *
 * The browser is whatever Playwright finds, unless `TESSERA_CHROMIUM` names one.
 * A container with Chromium preinstalled usually has a build number the pinned
 * Playwright does not expect, and pointing at the installed binary is what the
 * environment asks for instead of downloading a second copy.
 */

import { defineConfig, devices } from '@playwright/test';

const PORT = 8732;

const chromium = process.env.TESSERA_CHROMIUM;

export default defineConfig({
  testDir: './tests',
  // The core answers a card synchronously and the mock provider is instant, but
  // a cold binary and a first paint are not, so the default 30s stands.
  fullyParallel: false,
  // One core, one board, one connection at a time. Parallel workers would queue
  // behind each other on the same board and read each other's cards.
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        ...(chromium ? { launchOptions: { executablePath: chromium } } : {}),
      },
    },
  ],
  webServer: {
    command: `cargo run -q -p tessera-core --bin tessera-ui-server -- --ui app/ui/dist --port ${PORT}`,
    cwd: '../..',
    url: `http://127.0.0.1:${PORT}/`,
    reuseExistingServer: false,
    timeout: 180_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
