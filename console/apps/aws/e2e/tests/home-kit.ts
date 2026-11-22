// What the Home, Connect, Live and direction boards' specs share: a real doze-aws for each browser (seeded as the boards' "larder"
// world, its apps called), the photograph of a page light and dark, and the small moves every spec makes.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { expect, test } from "./fixtures";
import { type Apps, seedHome, trafficHome } from "./home-seed";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-home");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45800);

export const world = { server: undefined as unknown as Running, browser: "", apps: undefined as unknown as Apps };
export const url = (path: string) => `${world.server.console}${path}`;

/** One doze-aws for the spec's tests, seeded and called as the apps of the world; `slot` keeps two specs' servers apart. */
export function serveWorld(slot: number, args: string[] = []) {
  test.describe.configure({ mode: "serial" });
  test.beforeAll(async ({ browserName }) => {
    world.browser = browserName;
    const port = BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[browserName] ?? 40) + slot * 4;
    world.server = await start(port, port + 1, { args });
    world.apps = seedHome(world.server.endpoint, world.server.account);
    await trafficHome(world.server.endpoint, world.server.account, world.apps);
  });
  test.afterAll(async () => {
    await world.server.stop();
  });
}

export const cli = (args: string[]) => {
  const r = aws(world.server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

/** Photographs the page as the board is: light, then dark, in Chromium only. */
export async function shot(page: Page, board: string) {
  if (world.browser !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
  const height = await page.evaluate(() => Math.max(document.documentElement.scrollHeight, window.innerHeight));
  await page.setViewportSize({ width: 1440, height });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(SHOTS, `${board}.light.png`) });
  await page.evaluate(() => document.documentElement.classList.add("dark"));
  await page.waitForTimeout(250);
  await page.screenshot({ path: join(SHOTS, `${board}.dark.png`) });
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await page.setViewportSize({ width: 1440, height: 900 });
}

export const go = async (page: Page, path: string, title: string | RegExp) => {
  await page.goto(url(path));
  await expect(page.locator("h1").first()).toHaveText(title);
};
