// What the Lab's specs share: a doze-aws of the test's own (so a rule bends no other test's calls and nothing else's rules are in the
// table), and the way the form's lists are filled.
import type { Page } from "@playwright/test";
import { aws, type Running, start } from "../server";
import { expect } from "./fixtures";

export type Cli = (args: string[], options?: { attempts?: number }) => { code: number; out: string };

/** A doze-aws of this test's own, stopped by its pid at the end; `args` are extra flags. */
export async function own(page: Page, run: (server: Running, cli: Cli) => Promise<void>, args: string[] = []): Promise<void> {
  const base = 24_000 + 2 * Math.floor(Math.random() * 1_000);
  const server = await start(base, base + 1, { args });
  try {
    await run(server, (a, options) => aws(server.endpoint, a, options));
  } finally {
    // The page leaves first: one that is still open would find the server gone and log it.
    await page.goto("about:blank");
    await server.stop();
  }
}

/** Adds `text` to the list named `label` in an open form (a multi-select with a search field): the first thing it offers, the name as typed or
 * the option it matches. The popover is closed again. */
export async function addTo(page: Page, label: string, text: string): Promise<void> {
  await page.getByRole("combobox", { name: label, exact: true }).click();
  await page.locator("[cmdk-input]").fill(text);
  await page.keyboard.press("Enter");
  await page.keyboard.press("Escape");
  await expect(page.locator("[cmdk-input]")).toHaveCount(0);
}

/** The rule dialog's sentence, as it is said now (after the form has been still for a moment). */
export async function sentence(page: Page): Promise<string> {
  await expect(page.getByTestId("rule-sentence")).not.toHaveAttribute("data-pending", /.*/);
  return (await page.getByTestId("rule-sentence-text").textContent()) ?? "";
}
