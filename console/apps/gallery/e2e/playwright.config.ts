// The browser gate of the shared components: the gallery, built, under both products' themes, light and dark, driven with a
// real mouse and keyboard in Firefox (the owner's Zen), WebKit and Chromium.
//
//   bun run build && bun run e2e          (in console/; the run serves apps/gallery/dist itself)
//   GALLERY_URL=http://127.0.0.1:5173 bun run e2e     against a gallery that is already serving (bun run dev)
//
// One browser at a time (workers: 1): the machine is shared.
import { defineConfig, devices } from "@playwright/test";

const PORT = Number(process.env.GALLERY_PORT ?? 41731);
const baseURL = process.env.GALLERY_URL ?? `http://127.0.0.1:${PORT}`;
const viewport = { width: 1440, height: 900 };

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 45_000,
  expect: { timeout: 8_000 },
  reporter: [["list"]],
  outputDir: "../test-results",
  webServer: process.env.GALLERY_URL
    ? undefined
    : { command: `bunx --bun vite preview --host 127.0.0.1 --port ${PORT} --strictPort`, cwd: "..", url: baseURL, reuseExistingServer: false, timeout: 30_000 },
  use: { baseURL, viewport, trace: "off", screenshot: "off", actionTimeout: 8_000 },
  projects: [
    { name: "firefox", use: { ...devices["Desktop Firefox"], viewport } },
    { name: "webkit", use: { ...devices["Desktop Safari"], viewport } },
    { name: "chromium", use: { ...devices["Desktop Chrome"], viewport, permissions: ["clipboard-read", "clipboard-write"] } },
  ],
});
