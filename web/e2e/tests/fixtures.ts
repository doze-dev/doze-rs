import { test as base, expect, type Page } from "@playwright/test";

/** Every test fails on a console error or an uncaught exception in the page (the owner's bar: no console errors). */
export const test = base.extend<{ errors: string[] }>({
  errors: [
    async ({ page }, use) => {
      const errors: string[] = [];
      page.on("console", (m) => {
        // A font download that the next navigation cut short (Firefox: NS_BINDING_ABORTED, 0x804b0002) is the test moving
        // faster than a person, not a fault of the page.
        if (m.type() === "error" && !/downloadable font: download failed.*status=2152398850/.test(m.text())) errors.push(`console: ${m.text()}`);
      });
      page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
      await use(errors);
      expect(errors, "no console errors").toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };

/** The page scrolls sideways by this many pixels (0 is the rule). */
export async function overflowX(page: Page): Promise<number> {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

/** The table headers on the page whose words do not fit their column (cut with an ellipsis or spilling over): none is the rule
 * at 1280 px and wider. */
export async function cutHeaders(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    [...document.querySelectorAll("th")]
      .filter((th) => {
        const r = th.getBoundingClientRect();
        if (r.width === 0 || r.height === 0) return false;
        const inner = th.querySelector<HTMLElement>("button span") ?? th;
        return th.scrollWidth > th.clientWidth + 1 || inner.scrollWidth > inner.clientWidth + 1;
      })
      .map((th) => th.textContent?.trim() ?? ""),
  );
}

/** Opens a shadcn Select by its trigger and chooses an option by its visible name, with the mouse. */
export async function choose(page: Page, trigger: ReturnType<Page["locator"]>, option: string) {
  await trigger.click();
  await page.getByRole("option", { name: option, exact: true }).click();
}

/** The rectangles of the given locators, rounded. */
export async function boxes(page: Page, selectors: string[]) {
  return page.evaluate(
    (sels) =>
      sels.map((s) => {
        const r = document.querySelector(s)?.getBoundingClientRect();
        return r ? [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)] : null;
      }),
    selectors,
  );
}
