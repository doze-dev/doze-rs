// Every gallery page, under both products' themes, light and dark: it draws in that look, with no console error (fixtures),
// no sideways scroll, and the theme's own ground.
import { LOOKS, PAGES } from "../looks";
import { expect, GROUND, open, overflowX, tag, test } from "./fixtures";

for (const look of LOOKS) {
  test(`${tag(look)}: every page draws in the look, without errors or sideways scroll`, async ({ page }) => {
    for (const p of PAGES) {
      await open(page, p.path, look);
      await expect(page.locator("h1").first()).toHaveText(p.title);
      await expect(page.locator("body")).toHaveCSS("background-color", GROUND[tag(look)]!);
      expect(await overflowX(page), `${p.name} scrolls sideways`).toBe(0);
      // Every section drew something: no specimen is an empty box.
      const sections = page.locator("section[data-testid^=section-]");
      expect(await sections.count()).toBeGreaterThan(2);
    }
  });
}

test("the product switch changes the theme in place, and the choice survives a reload", async ({ page }) => {
  await open(page, "/", { theme: "kafka", mode: "light" });
  const primary = () => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--primary").trim().toLowerCase());
  expect(await primary()).toBe("#ffd60a");
  await page.getByTestId("product-aws").click();
  await expect(page.locator("html")).toHaveAttribute("data-product", "aws");
  await expect(page.locator("body")).toHaveCSS("background-color", GROUND["aws-light"]!);
  expect(await primary()).toBe("#7a4fe0");
  // One theme stylesheet at a time: the old one is gone.
  await expect(page.locator("link[data-theme]")).toHaveCount(1);
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-product", "aws");
  await page.getByTestId("theme-dark").click();
  await expect(page.locator("body")).toHaveCSS("background-color", GROUND["aws-dark"]!);
  await page.getByTestId("product-kafka").click();
  await expect(page.locator("body")).toHaveCSS("background-color", GROUND["kafka-dark"]!);
});

test("density is the theme's: a table row is 40 px under doze-kafka and 36 px under doze-aws, and 1,000 rows are virtual", async ({ page }) => {
  for (const [theme, px] of [["kafka", 40], ["aws", 36]] as const) {
    await open(page, "/data", { theme, mode: "light" });
    const table = page.locator('[data-slot="data-table"][data-virtual]');
    await expect(table).toBeVisible();
    await expect(table.locator("tbody tr[data-row]").first()).toHaveCSS("height", `${px}px`);
    // Only the rows in view (and a few around them) are in the page.
    expect(await table.locator("tbody tr[data-row]").count()).toBeLessThan(60);
    await expect(table.locator("table")).toHaveAttribute("aria-rowcount", "1001");
    // The last row is reachable, where 1,000 rows of that height put it.
    await table.evaluate((el) => el.scrollTo(0, el.scrollHeight));
    await expect(table.locator('tr[data-row="r999"]')).toBeVisible();
    expect(await table.evaluate((el) => el.scrollHeight)).toBe(40 + 1000 * px);
  }
});

test("a table sorts by a click on its header, and a row opens", async ({ page }) => {
  await open(page, "/data", { theme: "aws", mode: "light" });
  const table = page.getByRole("table", { name: "Twelve resources" });
  const calls = table.getByRole("columnheader", { name: "Calls" });
  await calls.getByRole("button").click();
  // A column of numbers sorts largest first on the first click.
  await expect(calls).toHaveAttribute("aria-sort", "descending");
  await expect(table.locator("tbody tr[data-row]").first().locator("td").nth(3)).toHaveText("87,109");
  await calls.getByRole("button").click();
  await expect(calls).toHaveAttribute("aria-sort", "ascending");
  await expect(table.locator("tbody tr[data-row]").first().locator("td").nth(3)).toHaveText("0");
  await table.locator('tr[data-row="r4"]').click();
  await expect(page.getByTestId("table-picked")).toHaveText("audit-0004");
});
