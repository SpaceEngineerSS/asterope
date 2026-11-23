import { defineConfig, devices } from '@playwright/test';

const port = 4173;
// Set PW_CHANNEL=msedge (or chrome) to use an installed browser instead of
// the Chromium that `npx playwright install chromium` downloads.
const channel = process.env.PW_CHANNEL;

export default defineConfig({
  testDir: 'e2e',
  timeout: 180_000,
  expect: { timeout: 30_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: `http://localhost:${port}/`,
    viewport: { width: 1600, height: 1000 },
  },
  // `npx playwright install chromium firefox webkit` fetches all three;
  // `--project chromium` runs one.
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1600, height: 1000 }, ...(channel ? { channel } : {}) } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'], viewport: { width: 1600, height: 1000 } } },
    { name: 'webkit', use: { ...devices['Desktop Safari'], viewport: { width: 1600, height: 1000 } } },
  ],
  webServer: {
    command: `npx vite preview --port ${port} --strictPort`,
    port,
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
