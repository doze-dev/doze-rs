import { expect, type Page } from "@playwright/test";

/** The page's breadcrumb ends as the boards' do: the page itself is the last crumb (plain text, aria-current), the parent before it a link.
 * `tail` is the last crumbs (`["Queues", "pick-lists"]`); an empty string stands for the page's own title, for a name the test does not spell. */
export async function expectCrumbsEnd(page: Page, tail: string[]): Promise<void> {
  const nav = page.getByRole("navigation", { name: "Breadcrumb" });
  const title = ((await page.locator("h1").first().innerText()) ?? "").trim();
  const want = tail.map((s) => (s === "" ? title : s));
  await expect(nav.locator("li").last()).toHaveText(want[want.length - 1]!);
  const all = (await nav.locator("li").allInnerTexts()).map((s) => s.trim());
  expect(all.slice(-want.length)).toEqual(want);
  await expect(nav.locator("span[aria-current=page]")).toHaveText(want[want.length - 1]!);
  // The one before it is still a way back.
  if (want.length > 1) await expect(nav.getByRole("link", { name: want[want.length - 2]!, exact: true }).first()).toBeVisible();
}
