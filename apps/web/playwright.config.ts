import { defineConfig, devices } from "@playwright/test";

const externalBaseUrl = process.env.LEAVE_E2E_BASE_URL?.trim();

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "github" : "line",
  use: {
    baseURL: externalBaseUrl || "http://127.0.0.1:4174",
    serviceWorkers: "block",
    trace: "on-first-retry"
  },
  projects: [
    {
      name: "chromium-375",
      use: { ...devices["Pixel 7"], viewport: { width: 375, height: 812 } }
    },
    {
      name: "chromium-812-landscape",
      use: { ...devices["Pixel 7"], viewport: { width: 812, height: 375 } }
    },
    {
      name: "chromium-768",
      use: { ...devices["Desktop Chrome"], viewport: { width: 768, height: 1024 } }
    },
    {
      name: "chromium-1024",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1024, height: 768 } }
    },
    {
      name: "chromium-1440",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } }
    },
    {
      name: "webkit-375",
      use: { ...devices["iPhone 15"], viewport: { width: 375, height: 812 } }
    },
    {
      name: "firefox-1440",
      use: { ...devices["Desktop Firefox"], viewport: { width: 1440, height: 900 } }
    }
  ],
  webServer: externalBaseUrl ? undefined : {
    command: "node ../../tests/fixtures/start-e2e-host.mjs",
    url: "http://127.0.0.1:4174/api/v1/local/status",
    reuseExistingServer: false,
    timeout: 30_000
  }
});
