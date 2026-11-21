import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test as base, expect, type Page } from "@playwright/test";
import type { About } from "../../src/api/types/About";
import type { CallsPage } from "../../src/api/types/CallsPage";
import type { ServicesResponse } from "../../src/api/types/ServicesResponse";
import { aws } from "../server";

/** The real doze-aws every test shares (e2e/global-setup.ts started it). */
export const ENDPOINT = process.env.DOZE_AWS_E2E_ENDPOINT ?? "";
export const CONSOLE = process.env.DOZE_AWS_E2E_CONSOLE ?? "";

const THEME = readFileSync(join(import.meta.dirname, "../../../../packages/ui/themes/aws.css"), "utf8");

/** A colour role of the doze-aws theme file, as the browser reports a colour (`rgb(r, g, b)`): read from the file, so the
 * tests hold whatever the theme's values are. `mode`: the `:root` block, or `:root.dark`. */
export function themeColour(mode: "light" | "dark", role: string): string {
  const block = (mode === "light" ? /^:root \{([\s\S]*?)^\}/m : /^:root\.dark \{([\s\S]*?)^\}/m).exec(THEME)?.[1] ?? "";
  const hex = new RegExp(`^\\s*${role}:\\s*#([0-9a-fA-F]{6})\\b`, "m").exec(block)?.[1];
  if (!hex) throw new Error(`themes/aws.css sets no hex ${role} for ${mode}`);
  return `rgb(${[0, 2, 4].map((i) => Number.parseInt(hex.slice(i, i + 2), 16)).join(", ")})`;
}

/** The page ground each mode must paint: the theme's --background. */
export const GROUND = { light: themeColour("light", "--background"), dark: themeColour("dark", "--background") } as const;

type Fixtures = {
  /** Console errors a test expects (it cuts a request, or stops the server): a pattern of the message. (One pattern, not a
   * list: Playwright reads an array given as an option as its `[value, options]` pair.) */
  allowErrors: RegExp | null;
  errors: string[];
  netlog: void;
  /** One call with the real AWS CLI against the shared doze-aws, signed with this browser's own access key. */
  cli: (args: string[], options?: { region?: string }) => { code: number; out: string };
  /** The access key this browser's calls are signed with (so a row is known to be this test's). */
  accessKey: string;
};

/** Every test fails on a console error or an uncaught exception in the page (the owner's bar: no console errors), except
 * the ones it says it expects. */
export const test = base.extend<Fixtures>({
  allowErrors: [null, { option: true }],
  errors: [
    async ({ page, allowErrors }, use) => {
      const errors: string[] = [];
      const expected = (text: string) =>
        // A font download that the next navigation cut short (Firefox: NS_BINDING_ABORTED) is the test moving faster than a
        // person, not a fault of the page.
        /downloadable font: download failed.*status=2152398850/.test(text) || (allowErrors?.test(text) ?? false);
      page.on("console", (m) => {
        if (m.type() === "error" && !expected(m.text())) errors.push(`console: ${m.text()}`);
      });
      page.on("pageerror", (e) => {
        if (!expected(e.message)) errors.push(`pageerror: ${e.message}`);
      });
      await use(errors);
      expect(errors, "no console errors").toEqual([]);
    },
    { auto: true },
  ],
  // What the page's network did that was not an answer of 2xx or 3xx, and every request that failed, attached to a test that
  // failed: the log that tells a refused connection (503) from a slow page.
  netlog: [
    async ({ page }, provide, testInfo) => {
      const lines: string[] = [];
      const at = () => `${((Date.now() - started) / 1000).toFixed(2)}s`;
      const started = Date.now();
      page.on("response", (r) => {
        if (r.status() >= 400 || r.request().method() !== "GET") lines.push(`${at()} ${r.status()} ${r.request().method()} ${r.url()}`);
      });
      page.on("request", (r) => { if (r.method() !== "GET") lines.push(`${at()} -> ${r.method()} ${r.url()}`); });
      page.on("requestfailed", (r) => lines.push(`${at()} FAILED ${r.method()} ${r.url()}: ${r.failure()?.errorText ?? ""}`));
      await provide();
      if (testInfo.status !== testInfo.expectedStatus && lines.length > 0) {
        await testInfo.attach("netlog", { body: lines.join("\n"), contentType: "text/plain" });
        console.log(`netlog of "${testInfo.title}":\n${lines.join("\n")}`);
      }
    },
    { auto: true },
  ],
  // (`provide`, not Playwright's usual `use`: the React hooks lint takes a call of `use` for the hook.)
  accessKey: async ({ browserName }, provide) => {
    await provide(`AKIAE2E${browserName.toUpperCase().padEnd(13, "0").slice(0, 13)}`);
  },
  cli: async ({ accessKey }, provide) => {
    await provide((args, options = {}) => aws(ENDPOINT, args, { accessKey, region: options.region }));
  },
});

export { expect };

/** What a request the browser could not finish logs, by browser: the tests that cut a request or stop the server expect it. */
export const CUT = /Failed to load resource|NetworkError|Load failed|net::ERR_|Could not connect|access control checks|network connection was lost/i;

const proof = { "X-Console": "1" };

/** The API, asked as the console's own client asks it (the proof header): what a page is compared with. */
export async function about(page: Page, consoleUrl = CONSOLE): Promise<About> {
  return (await (await page.request.get(`${consoleUrl}/api/v1/about`, { headers: proof })).json()) as About;
}
export async function services(page: Page): Promise<ServicesResponse> {
  return (await (await page.request.get(`${CONSOLE}/api/v1/services`, { headers: proof })).json()) as ServicesResponse;
}
export async function calls(page: Page, consoleUrl = CONSOLE): Promise<CallsPage> {
  return (await (await page.request.get(`${consoleUrl}/api/v1/calls?limit=100000`, { headers: proof })).json()) as CallsPage;
}

/** Opens a page of the console and waits for its title. */
export async function open(page: Page, path: string, title: string) {
  await page.goto(path);
  await expect(page.locator("h1").first()).toHaveText(title);
}

/** Opens Live and waits until its stream is open. */
export async function openLive(page: Page, path = "/live") {
  await open(page, path, "Live");
  await expect(page.getByTestId("live-status")).toHaveText("Live");
}

/** The rows of Live's table, newest first. */
export const rows = (page: Page) => page.locator('[data-slot="data-table"] tbody tr[data-row]');

/** The page scrolls sideways by this many pixels (0 is the rule). */
export async function overflowX(page: Page): Promise<number> {
  // Never below 0: with classic scrollbars (Chromium on Linux) the client width can exceed the scroll width by the bar.
  return page.evaluate(() => Math.max(0, document.documentElement.scrollWidth - document.documentElement.clientWidth));
}

/** A name no other test or browser uses. */
export const unique = (prefix: string, browserName: string) => `${prefix}-${browserName}-${Date.now().toString(36)}`;
