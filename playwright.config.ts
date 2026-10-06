import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/ui',
  use: { channel: 'chrome', baseURL: 'http://127.0.0.1:47832', viewport: { width: 1280, height: 900 } },
  webServer: { command: 'npm run dev -- --port 47832', url: 'http://127.0.0.1:47832', reuseExistingServer: false },
});
