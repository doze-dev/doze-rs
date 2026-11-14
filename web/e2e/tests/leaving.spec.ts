// A page that starts a request while it is being left (J500, CON-1514). Between `beforeunload` and the next document coming in,
// WebKit refuses every request the old page starts and logs "Fetch API cannot load <url> due to access control checks.", which
// Playwright reports as a page error, whether or not the page handles the rejection. A person sees it in Safari's console when
// a refresh or a tail starts just as they reload; the e2e saw it when a test went to a second address while the first one's
// queries were starting (live.spec.ts, "a broker that goes away and comes back"). The console holds a request started then
// until the page has gone (never sent) or stayed (sent).
//
// Each case makes the window wide and certain: the next document is held 700 ms by the test, and the request is started 20 ms
// into it by the page's own control. No error of any kind, console or page, is allowed: nothing is filtered here.
import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Holds the next document (`/connect?away=<n>`) for 700 ms, then lets it through. */
async function slowAway(page: Page) {
  await page.route(/\/connect\?away=\d+$/, async (route) => {
    await sleep(700);
    await route.continue();
  });
}

/** When the page starts to be left, clicks `selector` 20 ms later, as a person or a timer could. */
async function clickWhileLeaving(page: Page, selector: string) {
  await page.evaluate((sel) => {
    addEventListener("beforeunload", () => setTimeout(() => document.querySelector<HTMLElement>(sel)?.click(), 20));
  }, selector);
}

async function leave(page: Page, n: number) {
  await page.goto(`/connect?away=${n}`);
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
  // Anything the old page logs late arrives before the fixture's check.
  await sleep(200);
}

test("Refresh pressed as Home is left logs nothing",async ({ page }) => {
  await slowAway(page);
  for (let i = 0; i < 3; i++) {
    await page.goto("/");
    await expect(page.getByTestId("refresh-now")).toBeVisible();
    await clickWhileLeaving(page, "[data-testid=refresh-now]");
    await leave(page, i);
  }
});

test("the Live tail switched on as Live is left logs nothing", async ({ page }) => {
  await slowAway(page);
  for (let i = 0; i < 3; i++) {
    await page.goto("/live");
    await expect(page.getByTestId("tail-state")).toHaveText("Snapshot");
    await clickWhileLeaving(page, "[data-testid=tail-mode] [role=radio]:not([aria-checked=true])");
    await leave(page, i);
  }
});

test("a topic's Live tail switched on as Messages is left logs nothing", async ({ page }) => {
  await slowAway(page);
  for (let i = 0; i < 3; i++) {
    await page.goto("/topics/orders/messages");
    await expect(page.getByTestId("mode")).toBeVisible();
    await clickWhileLeaving(page, "[data-testid=mode] [role=radio]:not([aria-checked=true])");
    await leave(page, i);
  }
});

test("a page opened from the sidebar as the console is left (its code and its queries) logs nothing", async ({ page }) => {
  await slowAway(page);
  for (const [i, to] of ["/settings", "/lab", "/groups"].entries()) {
    await page.goto("/");
    await expect(page.locator(`a[href="${to}"]`).first()).toBeVisible();
    await clickWhileLeaving(page, `a[href="${to}"]`);
    await leave(page, i);
  }
});

test("going to a second address while the first one's queries start logs nothing", async ({ page, browserName }) => {
  test.setTimeout(90_000);
  // The race the e2e lost by chance, as live.spec.ts runs it: the next address asked for just as the first page's queries
  // (shell, live) start. Firefox drops about one goto in nine at this pace (NS_BINDING_ABORTED or NS_ERROR_FAILURE; measured
  // 14 of 120 with the console's beforeunload listener and 14 of 120 without it): Playwright's report of a navigation Firefox
  // replaced, not what this test is about, so the loop goes on and the page is still checked for errors.
  const go = async (url: string) => {
    try {
      await page.goto(url);
    } catch (e) {
      if (browserName !== "firefox" || !/NS_BINDING_ABORTED|NS_ERROR_FAILURE/.test((e as Error).message)) throw e;
    }
  };
  for (let i = 0; i < 15; i++) {
    await go("/live");
    await page.evaluate(() => localStorage.setItem("doze:tail-speed", "realtime"));
    await go("/live?tail=1");
  }
  await sleep(300);
  await page.goto("/live?tail=1");
  await expect(page.getByTestId("tail-state")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
});

test("a request started while leaving goes out after all when the page stays", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("refresh-now")).toBeVisible();
  const home = page.waitForResponse((r) => r.url().endsWith("/api/v1/home"), { timeout: 8_000 });
  // A beforeunload with no page after it (a navigation that never came): the hold lets go on its own.
  await page.evaluate(() => {
    dispatchEvent(new Event("beforeunload"));
    document.querySelector<HTMLElement>("[data-testid=refresh-now]")?.click();
  });
  expect((await home).ok()).toBe(true);
});
