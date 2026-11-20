// Keyboard focus is always visible, under every look: the focus arrives by Tab (never by script), and the element it lands
// on shows a ring in the theme's colour.
import { LOOKS } from "../looks";
import { expect, open, ringOf, tabKey, tabTo, tag, test } from "./fixtures";

for (const look of LOOKS) {
  test(`${tag(look)}: the focus is visible on a button, a field, a toggle, a table header and the frame`, async ({ page, browserName }) => {
    await open(page, "/actions", look);
    // From the top of the page: the frame first (the brand link, the rail, the top bar), then the page's controls.
    const seen: string[] = [];
    const primary = page.getByTestId("btn-default");
    for (let i = 0; i < 40; i++) {
      await page.keyboard.press(tabKey(browserName));
      const here = page.locator(":focus");
      const ring = await ringOf(here);
      const name = await here.evaluate((el) => el.getAttribute("data-testid") ?? el.getAttribute("aria-label") ?? el.textContent?.trim().slice(0, 30) ?? el.tagName);
      expect(ring.visible, `no focus ring on "${name}" (box-shadow: ${ring.shadow}; outline: ${ring.outline})`).toBe(true);
      seen.push(name);
      if (await primary.evaluate((el) => el === document.activeElement)) break;
    }
    await expect(primary).toBeFocused();
    // The frame's controls were on the way: the rail's links, the palette's trigger, the product and theme switches.
    expect(seen).toContain("palette-trigger");
    expect(seen.some((s) => s.startsWith("product-"))).toBe(true);
    expect(seen.some((s) => s.startsWith("theme-"))).toBe(true);
    // Space presses the focused button.
    await page.keyboard.press("Space");
    await expect(page.getByTestId("btn-clicks")).toHaveText("1 pressed");

    const toggle = page.getByTestId("toggle-bold");
    await tabTo(page, browserName, toggle);
    expect((await ringOf(toggle)).visible).toBe(true);
    await page.keyboard.press("Space");
    await expect(toggle).toHaveAttribute("aria-pressed", "true");

    await open(page, "/forms", look);
    const field = page.getByTestId("input-default");
    await tabTo(page, browserName, field);
    expect((await ringOf(field)).visible).toBe(true);
    await page.keyboard.type("orders");
    await expect(field).toHaveValue("orders");

    await open(page, "/data", look);
    const sort = page.getByRole("table", { name: "Twelve resources" }).getByRole("columnheader", { name: "Name" }).getByRole("button");
    await tabTo(page, browserName, sort);
    expect((await ringOf(sort)).visible).toBe(true);
    await page.keyboard.press("Enter");
    await expect(page.getByRole("table", { name: "Twelve resources" }).getByRole("columnheader", { name: "Name" })).toHaveAttribute("aria-sort", "ascending");
  });
}

test("reduced motion: an overlay opens and closes without moving", async ({ browser }) => {
  const ctx = await browser.newContext({ reducedMotion: "reduce", viewport: { width: 1440, height: 900 } });
  const page = await ctx.newPage();
  await page.goto("/overlays?theme=aws&mode=light");
  await page.getByTestId("open-sheet-r0").click();
  const sheet = page.getByTestId("sheet");
  await expect(sheet).toBeVisible();
  // The theme's 200 ms is cut to nothing by the base stylesheet.
  expect(Number.parseFloat(await sheet.evaluate((el) => getComputedStyle(el).animationDuration))).toBeLessThan(0.001);
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await ctx.close();
});

test("motion is the theme's: a drawer comes in over --dur-in and goes out over --dur-out", async ({ page }) => {
  for (const [theme, inS, outS] of [["kafka", 0.2, 0.15], ["aws", 0.2, 0.16]] as const) {
    await page.goto(`/overlays?theme=${theme}&mode=light`);
    await page.getByTestId("open-sheet-r0").click();
    const sheet = page.getByTestId("sheet");
    await expect(sheet).toBeVisible();
    expect(Number.parseFloat(await sheet.evaluate((el) => getComputedStyle(el).animationDuration))).toBeCloseTo(inS, 3);
    await sheet.evaluate((el) => el.setAttribute("data-state", "closed"));
    expect(Number.parseFloat(await sheet.evaluate((el) => getComputedStyle(el).animationDuration))).toBeCloseTo(outS, 3);
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();
  }
});
