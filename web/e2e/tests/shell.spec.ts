import { boxes, choose, expect, overflowX, test } from "./fixtures";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.goto("/");
  await expect(page.getByTestId("home")).toBeVisible();
});

test("the sidebar goes to every section and marks the one you are in", async ({ page }) => {
  const nav = page.getByTestId("sidebar");
  for (const [label, path] of [
    ["Live", "/live"],
    ["Topics", "/topics"],
    ["Groups", "/groups"],
    ["Access", "/access"],
    ["Lab", "/lab"],
    ["Settings", "/settings"],
    ["Home", "/"],
  ] as const) {
    await nav.getByRole("link", { name: label, exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`${path === "/" ? "/$" : `${path}$`}`));
    await expect(nav.getByRole("link", { name: label, exact: true })).toHaveAttribute("aria-current", "page");
    await expect(page.locator("h1")).toHaveCount(1);
  }
  // The counts are the broker's.
  await expect(nav.getByRole("listitem").filter({ hasText: "Topics" })).toContainText(/\d/);
});

test("the sidebar collapses to icons and back, and a collapsed item still navigates", async ({ page }) => {
  const sidebar = page.locator("[data-slot=sidebar][data-state]");
  await expect(sidebar).toHaveAttribute("data-state", "expanded");
  await page.getByRole("button", { name: "Toggle Sidebar" }).first().click();
  await expect(sidebar).toHaveAttribute("data-state", "collapsed");
  await expect(page.getByTestId("sidebar").getByText("Topics", { exact: true })).toBeHidden();
  await page.getByTestId("sidebar").getByRole("link", { name: "Topics" }).click();
  await expect(page).toHaveURL(/\/topics$/);
  await page.getByRole("button", { name: "Toggle Sidebar" }).first().click();
  await expect(sidebar).toHaveAttribute("data-state", "expanded");
});

test("the top bar: the address chip copies, the security chip opens Access", async ({ page }) => {
  const chip = page.getByTestId("address-chip");
  await expect(chip).toHaveText(/\d+\.\d+\.\d+\.\d+:\d+/);
  await chip.click();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Address copied" })).toBeVisible();
  await expect(page.getByTestId("profile-chip")).toHaveText(/Kafka \d+ profile/);
  await page.getByTestId("security-chip").click();
  await expect(page).toHaveURL(/\/access$/);
});

test("the palette opens with the keyboard, finds a topic by part of its name, goes there and remembers it", async ({ page }) => {
  await page.keyboard.press("ControlOrMeta+k");
  const input = page.getByTestId("palette-input");
  await expect(input).toBeFocused();
  await input.fill("ord");
  const item = page.getByRole("option", { name: "orders", exact: true });
  await expect(item).toBeVisible();
  await expect(page.getByRole("group", { name: "Topics" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Pages" })).toHaveCount(0);
  await item.click();
  await expect(page).toHaveURL(/\/topics\/orders$/);
  await expect(page.locator("h1")).toHaveText("orders");
  await expect(input).toBeHidden();
  // Recents
  await page.getByTestId("palette-trigger").click();
  await expect(page.getByRole("group", { name: "Recents" }).getByRole("option", { name: /orders/ })).toBeVisible();
  // Esc closes and gives the focus back to the trigger.
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-input")).toBeHidden();
  await expect(page.getByTestId("palette-trigger")).toBeFocused();
});

test("the palette goes to a page with the arrow keys and Enter, and says when nothing matches", async ({ page }) => {
  await page.getByTestId("palette-trigger").click();
  await page.getByTestId("palette-input").fill("gallery");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/gallery$/);
  await page.keyboard.press("ControlOrMeta+k");
  await page.getByTestId("palette-input").fill("zzz-nothing-is-called-this");
  await expect(page.getByTestId("palette-list")).toContainText("Nothing is called");
});

test("the refresh interval is chosen with the mouse, remembered across a reload, and Off stops the timer", async ({ page }) => {
  const trigger = page.getByTestId("refresh-interval");
  await expect(trigger).toHaveText("Refresh: Off");
  await choose(page, trigger, "Every 5 s");
  await expect(trigger).toHaveText("Every 5 s");
  expect(await page.evaluate(() => localStorage.getItem("doze:refresh"))).toBe("5s");
  await page.reload();
  await expect(page.getByTestId("refresh-interval")).toHaveText("Every 5 s");
  // The interval fires: the stamp moves on, and nothing on the page moves.
  const stamp = page.getByTestId("refresh-stamp");
  const first = await stamp.textContent();
  const selectors = ["h1", "[data-slot=stat-strip]", "[data-testid=home-callout]", "[data-testid=address-card]", "[data-testid=instance-card]", "[data-testid=next-card]"];
  const before = await boxes(page, selectors);
  await expect(stamp).not.toHaveText(first ?? "", { timeout: 12_000 });
  const after = await boxes(page, selectors);
  expect(after).toEqual(before);
  // With the keyboard this time: Off.
  await page.getByTestId("refresh-interval").focus();
  await page.keyboard.press("Enter");
  await page.getByRole("option", { name: "Refresh: Off" }).press("Enter");
  await expect(page.getByTestId("refresh-interval")).toHaveText("Refresh: Off");
  const still = await stamp.textContent();
  await page.waitForTimeout(6_000);
  await expect(stamp).toHaveText(still ?? "");
});

test("Refresh and the r key fetch the page again", async ({ page }) => {
  const stamp = page.getByTestId("refresh-stamp");
  await page.waitForTimeout(1_100);
  const first = await stamp.textContent();
  await page.getByTestId("refresh-now").click();
  await expect(stamp).not.toHaveText(first ?? "");
  await page.waitForTimeout(1_100);
  const second = await stamp.textContent();
  await page.locator("h1").click();
  await page.keyboard.press("r");
  await expect(stamp).not.toHaveText(second ?? "");
});

for (const width of [1024, 1280, 1440, 1920]) {
  test(`nothing scrolls sideways at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    for (const path of ["/", "/connect", "/gallery", "/gallery?rows=1000", "/topics", "/topics/a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2"]) {
      await page.goto(path);
      await expect(page.locator("h1")).toBeVisible();
      await page.waitForLoadState("networkidle");
      expect(await overflowX(page), `${path} at ${width}`).toBeLessThanOrEqual(0);
    }
  });
}

test("a deep link opens the page as it was, and the classic console is still there", async ({ page }) => {
  await page.goto("/connect?as=environment");
  await expect(page.getByTestId("copy-as").getByRole("radio", { name: "Environment" })).toHaveAttribute("aria-checked", "true");
  await page.goto("/no/such/page");
  await expect(page.locator("h1")).toHaveText("Not here");
  await page.getByTestId("sidebar").getByTestId("classic-link").click();
  await expect(page).toHaveURL(/\/classic\/$/);
  await expect(page.locator("h1")).toBeVisible();
});
