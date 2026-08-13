import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: 'http://127.0.0.1:4173',
    trace: 'retain-on-failure',
  },
  webServer: [
    {
      command: 'node ../scripts/start-e2e-service.mjs',
      url: 'http://127.0.0.1:18787/api/health',
      timeout: 120_000,
      reuseExistingServer: false,
    },
    {
      command: 'npm run dev:frontend -- --host 127.0.0.1 --port 4173',
      url: 'http://127.0.0.1:4173',
      timeout: 30_000,
      reuseExistingServer: false,
      env: { VITE_API_PORT: '18787' },
    },
  ],
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
