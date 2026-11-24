// The frame: the six spaces, the palette, the theme and the chips.
import { about, expect, GROUND, open, overflowX, test, themeColour } from "./fixtures";

const SPACES = [
  { label: "Home", path: "/" },
  { label: "Live", path: "/live" },
  { label: "Resources", path: "/resources" },
  { label: "Access", path: "/access" },
  { label: "Lab", path: "/lab" },
  { label: "Settings", path: "/settings" },
] as const;

test("the rail has the six spaces, each opens its page, and its address survives a reload", async ({ page }) => {
  await open(page, "/", "Home");
  const rail = page.getByRole("navigation", { name: "Spaces" });
  await expect(rail.getByRole("link")).toHaveText(SPACES.map((s) => s.label));
  for (const s of SPACES) {
    await rail.getByRole("link", { name: s.label, exact: true }).click();
    await expect(page.locator("h1").first()).toHaveText(s.label);
    await expect(page).toHaveURL(new RegExp(`${s.path === "/" ? "/" : s.path}$`));
    // One space is the current one.
    await expect(rail.locator('[aria-current="page"]')).toHaveText(s.label);
    expect(await overflowX(page), `${s.label} scrolls sideways`).toBe(0);
    // The server answers the app for a deep link: the same page after a reload.
    await page.reload();
    await expect(page.locator("h1").first()).toHaveText(s.label);
  }
  // The mark is the brand's file, served by doze-aws itself, and it drew.
  const mark = page.getByTestId("brand").locator("img");
  await expect(mark).toHaveAttribute("src", /\/assets\/doze-aws-[\w-]+\.svg$/);
  expect(await mark.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
});

test("the palette goes to a space and switches the theme, by the keyboard alone", async ({ page }) => {
  await open(page, "/", "Home");
  await page.keyboard.press("ControlOrMeta+k");
  const input = page.getByTestId("palette-input");
  await expect(input).toBeFocused();
  // The six spaces and the three theme choices are offered (and, under them, the resources of the shared doze-aws).
  await expect(page.getByTestId("palette-list").getByRole("group", { name: "Spaces" }).getByRole("option")).toHaveCount(6);
  await expect(page.getByTestId("palette-list").getByRole("group", { name: "Theme" }).getByRole("option")).toHaveCount(3);
  await input.fill("lab");
  await expect(page.getByTestId("palette-list").getByRole("option")).toHaveText(["Lab"]);
  await page.keyboard.press("Enter");
  await expect(page.locator("h1").first()).toHaveText("Lab");
  await expect(page).toHaveURL(/\/lab$/);

  await page.keyboard.press("ControlOrMeta+k");
  await page.getByTestId("palette-input").fill("dark");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("palette-input")).toHaveCount(0);
  await expect(page.locator("html")).toHaveClass(/\bdark\b/);
  await expect(page.locator("body")).toHaveCSS("background-color", GROUND.dark);
  // Switching the theme is not a place: the page did not change, and Recents does not offer it.
  await expect(page.locator("h1").first()).toHaveText("Lab");
  await page.keyboard.press("ControlOrMeta+k");
  await expect(page.getByTestId("palette-list").getByRole("group", { name: "Recents" }).getByRole("option")).toHaveText(["Labspace"]);
  await page.getByTestId("palette-input").fill("light");
  await page.keyboard.press("Enter");
  await expect(page.locator("body")).toHaveCSS("background-color", GROUND.light);
  // Esc closes it and the page is as it was.
  await page.getByTestId("palette-trigger").click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("palette-input")).toHaveCount(0);
});

test("light and dark are a choice that is kept across a reload", async ({ page }) => {
  await open(page, "/", "Home");
  await page.getByTestId("theme-dark").click();
  await expect(page.locator("body")).toHaveCSS("background-color", GROUND.dark);
  await page.reload();
  await expect(page.locator("h1").first()).toHaveText("Home");
  await expect(page.locator("html")).toHaveClass(/\bdark\b/);
  await expect(page.locator("body")).toHaveCSS("background-color", GROUND.dark);
  await expect(page.getByTestId("theme-dark")).toHaveAttribute("aria-checked", "true");
  // A card is the dark theme's too, not left light.
  await expect(page.getByTestId("card-connect")).toHaveCSS("background-color", themeColour("dark", "--card"));
  expect(GROUND.dark).not.toBe(GROUND.light);
  await page.getByTestId("theme-light").click();
  await page.reload();
  await expect(page.locator("html")).toHaveClass(/\blight\b/);
  await expect(page.locator("body")).toHaveCSS("background-color", GROUND.light);
});

test("the top bar says where SDKs connect, the region and what IAM does, as the server says them", async ({ page }) => {
  await open(page, "/", "Home");
  const a = await about(page);
  await expect(page.getByTestId("chip-endpoint")).toHaveText(a.endpoint.replace("http://", ""));
  await expect(page.getByTestId("chip-region")).toHaveText(a.region);
  await expect(page.getByTestId("chip-iam")).toHaveText(`IAM ${a.iam}${a.iam === "soft" ? " · recording" : ""}`);
});

test("the endpoint chip copies the endpoint", async ({ page, browserName }) => {
  // The clipboard can be read back in Chromium only (the other two give a test no permission for it): there this is not run.
  test.skip(browserName !== "chromium", "the clipboard is readable in Chromium only");
  await open(page, "/", "Home");
  const a = await about(page);
  await page.getByTestId("chip-endpoint").click();
  await expect(page.getByText("Endpoint copied")).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(a.endpoint);
});

test("an address the console has no page for says so and leads home", async ({ page }) => {
  await open(page, "/nowhere/at/all", "Not here");
  await page.getByRole("link", { name: "Home", exact: true }).last().click();
  await expect(page.locator("h1").first()).toHaveText("Home");
});

test("a press of Refresh is felt: the icon turns, a check shows that it came, and the time of the data takes the ink for a moment", async ({ page }) => {
  await open(page, "/", "Home");
  const button = page.getByTestId("refresh-now");
  await expect(button).toHaveAttribute("data-phase", "idle");
  await button.click();
  // The answer is local and quick, but the press is not over before it has been seen.
  await expect(button).toHaveAttribute("data-phase", "turning");
  await expect(button).toHaveAttribute("data-phase", "done");
  await expect(page.getByRole("status").filter({ hasText: "Refreshed" })).toHaveCount(1);
  await expect(button).toHaveAttribute("data-phase", "idle");
});
