// The door to other machines (SEC-250, CON-1360): once the API answers 403 CONSOLE_CLOSED_TO_NETWORK, the whole app is the page
// that says what happened, why and the two ways forward; Try again brings the console back once the server answers again. The
// harness's console is on loopback, so the refusal is the server's own words, played by the browser's router.
import { test as base, expect, type Page } from "@playwright/test";

const SAID =
  "This broker now has ACLs, so the console no longer answers other machines: it has no login and acts as a super user, so it would get round them. To go on, use it on the machine doze-kafka runs on, at http://127.0.0.1:9093/, or restart doze-kafka with --console-unprotected to accept that.";

/** No console error but the refused requests themselves (a browser logs a 403 it was answered). */
const test = base.extend<{ errors: string[] }>({
  errors: [
    async ({ page }, use) => {
      const errors: string[] = [];
      page.on("console", (m) => {
        if (m.type() === "error" && !/status of 403|403 \(Forbidden\)|downloadable font/.test(m.text())) errors.push(`console: ${m.text()}`);
      });
      page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
      await use(errors);
      expect(errors, "no console errors").toEqual([]);
    },
    { auto: true },
  ],
});

async function closeDoor(page: Page) {
  await page.route("**/api/v1/**", (route) =>
    route.fulfill({ status: 403, contentType: "application/json; charset=utf-8", body: JSON.stringify({ code: "CONSOLE_CLOSED_TO_NETWORK", message: SAID }) }),
  );
}

test("a console closed to this machine is one calm page: what happened, why, the two ways forward", async ({ page }) => {
  await closeDoor(page);
  await page.goto("/topics");
  const closed = page.getByTestId("closed-to-network");
  await expect(closed).toBeVisible();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("This console no longer answers other machines");
  await expect(closed).toContainText("What happened.");
  await expect(closed).toContainText("Why it stopped.");
  await expect(closed).toContainText("acts as a super user");
  await expect(closed.getByRole("heading", { level: 2 })).toHaveText(/Two ways forward/i);
  await expect(closed.getByRole("listitem")).toHaveCount(2);
  await expect(closed).toContainText("--console-unprotected");
  await expect(page.getByTestId("closed-said")).toHaveText(SAID);
  // Nothing of the app behind it: no sidebar, no top bar, no toast; one main landmark; no sideways scroll.
  await expect(page.getByTestId("sidebar")).toHaveCount(0);
  await expect(page.getByTestId("topbar")).toHaveCount(0);
  await expect(page.locator("[data-sonner-toast]")).toHaveCount(0);
  await expect(page.getByRole("main")).toHaveCount(1);
  expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBe(0);
});

test("the app gives way to the page when the door closes, and comes back with Try again once it opens", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("home")).toBeVisible();
  await closeDoor(page);
  await page.getByTestId("sidebar").getByRole("link", { name: "Topics", exact: true }).click();
  await expect(page.getByTestId("closed-to-network")).toBeVisible();
  await page.unroute("**/api/v1/**");
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByTestId("closed-to-network")).toHaveCount(0);
  await expect(page.getByTestId("sidebar")).toBeVisible();
  await expect(page).toHaveURL(/\/topics$/);
});
