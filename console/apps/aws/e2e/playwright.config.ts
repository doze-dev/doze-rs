// The browser gate of the doze-aws console: the built bundle, served by a REAL doze-aws (e2e/global-setup.ts starts it), called
// with the real AWS CLI, in Firefox (the owner's Zen), WebKit and Chromium.
//
//   bun run build                                  (in console/apps/aws: writes crates/aws-console/web-dist)
//   cargo build -p doze-aws                        (embeds it)
//   bun run e2e                                    (in console/apps/aws, or console/ for the gallery too)
//
// One browser at a time (workers: 1): the tests share one doze-aws and its Live record, and the machine is shared.
import { defineConfig, devices } from "@playwright/test";
import { CONSOLE_PORT } from "./global-setup";

const baseURL = `http://127.0.0.1:${CONSOLE_PORT}`;
const viewport = { width: 1440, height: 900 };

export default defineConfig({
  testDir: "./tests",
  globalSetup: "./global-setup.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: [["list"]],
  outputDir: "../test-results",
  use: { baseURL, viewport, trace: "off", screenshot: "off", actionTimeout: 8_000 },
  projects: [
    { name: "firefox", use: { ...devices["Desktop Firefox"], viewport } },
    { name: "webkit", use: { ...devices["Desktop Safari"], viewport } },
    { name: "chromium", use: { ...devices["Desktop Chrome"], viewport, permissions: ["clipboard-read", "clipboard-write"] } },
  ],
});
