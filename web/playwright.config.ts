import { defineConfig, devices } from '@playwright/test';

// End-to-end tests drive the real binaries (see e2e/support/stack.ts), so build
// them first: `cargo build -p photoframe-web -p photoframe-indexer`.
//
//   npm run e2e                     # both browsers (CI)
//   npm run e2e -- --project=chromium
//
// In a container without Playwright's own browsers, point PW_CHROMIUM at a
// system Chromium (for example /usr/bin/chromium) and add PW_NO_SANDBOX=1.
const chromiumPath = process.env.PW_CHROMIUM;
const noSandbox = !!process.env.PW_NO_SANDBOX;

export default defineConfig({
  testDir: 'e2e',
  timeout: 90_000,
  expect: { timeout: 10_000 },
  workers: 2,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['list'], ['github']] : [['list']],
  use: { trace: 'retain-on-failure', viewport: { width: 1280, height: 800 } },
  projects: [
    {
      name: 'chromium',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 1280, height: 800 },
        launchOptions: { executablePath: chromiumPath, args: noSandbox ? ['--no-sandbox'] : [] },
      },
    },
    { name: 'firefox', use: { ...devices['Desktop Firefox'], viewport: { width: 1280, height: 800 } } },
  ],
});
