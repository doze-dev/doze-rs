// What the KMS, Secrets Manager and Parameter Store board specs share: the photographs (light, then dark, Chromium only), a page opened
// by its heading, and a real doze-aws per browser on ports of the spec's own (E2E_BASE_PORT, default 46200).
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, type Running } from "../server";
import { expect } from "./fixtures";

export const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-kmsetc");
const BASE = Number(process.env.E2E_BASE_PORT ?? 46200);
/** The two ports of a spec's server (`slot` 0 to 7 apart), by browser. */
export const portsFor = (browser: string, slot: number): [number, number] => {
  const p = BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[browser] ?? 40) + slot * 2;
  return [p, p + 1];
};

export function cliOf(server: Running) {
  return (args: string[]): string => {
    const r = aws(server.endpoint, args);
    expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
    return r.out;
  };
}

/** The page in one mode: the media preference and the classes the theme sets on <html> (the page's listener may not have run yet). */
async function mode(page: Page, theme: "light" | "dark") {
  await page.emulateMedia({ colorScheme: theme });
  await page.evaluate((t) => { const el = document.documentElement; el.classList.toggle("dark", t === "dark"); el.classList.toggle("light", t === "light"); }, theme);
  await page.waitForTimeout(250);
}

/** Photographs the page as the board is: light, then dark, in Chromium only, the whole page as one tall viewport. */
export async function shoot(page: Page, browser: string, board: string) {
  if (browser !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
  const height = await page.evaluate(() => Math.max(document.documentElement.scrollHeight, window.innerHeight));
  await page.setViewportSize({ width: 1440, height: Math.min(height, 5000) });
  await mode(page, "light");
  await page.screenshot({ path: join(SHOTS, `${board}.light.png`) });
  await mode(page, "dark");
  await page.screenshot({ path: join(SHOTS, `${board}.dark.png`) });
  await mode(page, "light");
  await page.setViewportSize({ width: 1440, height: 900 });
}

export async function open(page: Page, consoleUrl: string, path: string, title: string | RegExp) {
  await page.goto(`${consoleUrl}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
}

/** The id of the key an alias points at. */
export function keyOf(cli: (a: string[]) => string, alias: string): string {
  const out = JSON.parse(cli(["kms", "list-aliases"])) as { Aliases: { AliasName: string; TargetKeyId: string }[] };
  return out.Aliases.find((a) => a.AliasName === `alias/${alias}`)!.TargetKeyId;
}
