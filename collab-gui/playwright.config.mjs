import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  reporter: 'list',
  use: {
    baseURL: 'http://localhost:1421',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'node dev-server.js',
    url: 'http://localhost:1421',
    reuseExistingServer: !process.env.CI,
    stdout: 'ignore',
    stderr: 'pipe',
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' } },
  ],
});
