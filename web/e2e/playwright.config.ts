// The browser gate of the React console (docs/port/console/react-plan.md, "Verification"): every control driven with a real
// mouse and keyboard in Firefox (the owner's Zen), WebKit and Chromium, against the real doze-kafka binary.
//
//   bun run test                      starts target/release/doze-kafka on a fresh data dir with a seeded world (global-setup.ts)
//   E2E_URL=http://127.0.0.1:29094 bun run test    runs against a console that is already serving (a broker with traffic)
//
// One browser at a time (workers: 1): the machine is shared.
import { defineConfig, devices } from "@playwright/test";

export const SEEDED = { kafka: "127.0.0.1:39092", console: "http://127.0.0.1:39094" };
const baseURL = process.env.E2E_URL ?? SEEDED.console;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 45_000,
  expect: { timeout: 8_000 },
  reporter: [["list"]],
  globalSetup: process.env.E2E_URL ? undefined : "./global-setup.ts",
  use: {
    baseURL,
    viewport: { width: 1440, height: 900 },
    trace: "off",
    screenshot: "off",
    actionTimeout: 8_000,
  },
  projects: [
    { name: "firefox", use: { ...devices["Desktop Firefox"], viewport: { width: 1440, height: 900 } } },
    { name: "webkit", use: { ...devices["Desktop Safari"], viewport: { width: 1440, height: 900 } } },
    { name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 }, permissions: ["clipboard-read", "clipboard-write"] } },
  ],
});
