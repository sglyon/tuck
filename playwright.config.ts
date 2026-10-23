import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests/ui",
  timeout: 30000,
  use: {
    baseURL: "http://127.0.0.1:1420",
    viewport: { width: 1320, height: 900 },
    launchOptions: {
      ...(process.env.TUCK_CHROME_PATH
        ? { executablePath: process.env.TUCK_CHROME_PATH }
        : process.platform === "darwin"
          ? {
              executablePath:
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            }
          : {}),
    },
  },
  webServer: {
    command: "npm run dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: true,
  },
  reporter: "list",
});
