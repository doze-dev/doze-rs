// What the API Gateway board specs share: the doze-aws of one browser (with its own data, seeded as "larder"), the CLI against it, the shots in
// Chromium (light and dark), and the small moves the pages ask for (a Select, a combobox, a page by its title).
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { type Larder, seedLarder } from "./apigateway-seed";
import { expect, pickFrom } from "./fixtures";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-apigateway");
// This slice's ports: 45900-45999 (the shared server of the run is at 45990).
const BASE = Number(process.env.E2E_BASE_PORT ?? 45900);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

export const ctx: { server: Running; kind: string; ids: Larder } = { server: undefined as never, kind: "", ids: undefined as never };

export async function begin(browserName: string): Promise<void> {
  ctx.kind = browserName;
  ctx.server = await start(at(browserName), at(browserName) + 1);
  ctx.ids = seedLarder(ctx.server.endpoint, ctx.server.account);
}
export const end = () => ctx.server.stop();

export const cli = (args: string[]) => {
  const r = aws(ctx.server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};
export const json = <T>(args: string[]) => JSON.parse(cli(args)) as T;

/** Photographs the page as the board is: light, then dark (the class the theme switch sets), in Chromium only. */
export async function shot(page: Page, board: string) {
  if (ctx.kind !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
  const height = await page.evaluate(() => Math.max(document.documentElement.scrollHeight, window.innerHeight));
  await page.setViewportSize({ width: 1440, height });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(SHOTS, `apigateway__${board}.light.png`) });
  await page.evaluate(() => document.documentElement.classList.add("dark"));
  await page.waitForTimeout(250);
  await page.screenshot({ path: join(SHOTS, `apigateway__${board}.dark.png`) });
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await page.setViewportSize({ width: 1440, height: 900 });
}

export const go = async (page: Page, path: string, title: string | RegExp) => {
  await page.goto(`${ctx.server.console}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
};
export const apiPath = (kind: "restapis" | "apis" | "usageplans", id: string, q = "") => `/resources/apigateway/${kind}/${id}${q}`;

/** Chooses `option` in the combobox named `name`. */
export const pick = async (page: Page, name: string | RegExp, option: string | RegExp) => {
  await pickFrom(page, name, option);
};
/** Chooses `option` in the Select whose trigger has `testId`. */
export const choose = async (page: Page, testId: string, option: string | RegExp) => {
  await page.getByTestId(testId).click();
  await page.getByRole("option", { name: option, exact: typeof option === "string" }).click();
};
