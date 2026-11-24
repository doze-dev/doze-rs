// What the CloudWatch boards' specs share: the real doze-aws each browser gets (seeded as the boards' "larder" world), the photograph
// of a page light and dark, and the small moves every spec makes.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { expect, test, pickFrom } from "./fixtures";
import { seedLarderCloudwatch } from "./cloudwatch-seed";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-cloudwatch");
const BASE = Number(process.env.E2E_BASE_PORT ?? 46100);

export const world = { server: undefined as unknown as Running, browser: "" };
export const url = (path: string) => `${world.server.console}${path}`;

/** One doze-aws for the spec's tests, seeded as the boards' world; `slot` keeps two specs' servers on ports of their own. */
export function serveWorld(slot: number) {
  test.describe.configure({ mode: "serial" });
  test.beforeAll(async ({ browserName }) => {
    world.browser = browserName;
    const port = BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[browserName] ?? 40) + slot * 4;
    world.server = await start(port, port + 1);
    seedLarderCloudwatch(world.server.endpoint, world.server.account);
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
export const alarm = (name: string, search = "") => `/resources/cloudwatch/alarms/${name}${search}`;
export const dash = (name: string, search = "") => `/resources/cloudwatch/dashboards/${name}${search}`;
export const choose = async (page: Page, combobox: string, option: string | RegExp) => {
  await pickFrom(page, combobox, option, { exact: false, first: true, nameExact: true });
};
export const pick = async (page: Page, trigger: string, option: string | RegExp) => {
  await page.getByTestId(trigger).click();
  await page.getByRole("option", { name: option }).click();
};
export const code = (page: Page) => page.getByTestId("call-panel-code");
