import { expect, test } from "./fixtures";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await page.evaluate(() => localStorage.clear());
  await page.goto("/");
  await expect(page.getByTestId("home")).toBeVisible();
});

test("the theme switch: System by default, a click picks Light or Dark, the choice survives a reload, and arrows move it", async ({ page }) => {
  const html = page.locator("html");
  const sw = page.getByTestId("theme-switch");
  await expect(sw.getByRole("radio", { name: "System" })).toHaveAttribute("aria-checked", "true");

  await sw.getByRole("radio", { name: "Dark" }).click();
  await expect(html).toHaveClass(/\bdark\b/);
  await expect(sw.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
  const darkBg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);

  await page.reload();
  await expect(html).toHaveClass(/\bdark\b/);
  await expect(page.getByTestId("theme-switch").getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");

  // Clicking the chosen one again keeps it (one is always chosen).
  await page.getByTestId("theme-dark").click();
  await expect(html).toHaveClass(/\bdark\b/);

  await page.getByTestId("theme-light").click();
  await expect(html).toHaveClass(/\blight\b/);
  await expect(html).not.toHaveClass(/\bdark\b/);
  const lightBg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  expect(lightBg).not.toBe(darkBg);

  // The keyboard: the arrows move the choice within the group.
  await page.getByTestId("theme-light").focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByTestId("theme-dark")).toBeFocused();
  await page.keyboard.press("Space");
  await expect(html).toHaveClass(/\bdark\b/);
});

test("System follows the OS", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.reload();
  await expect(page.locator("html")).toHaveClass(/\bdark\b/);
  await page.emulateMedia({ colorScheme: "light" });
  await expect(page.locator("html")).toHaveClass(/\blight\b/);
});
