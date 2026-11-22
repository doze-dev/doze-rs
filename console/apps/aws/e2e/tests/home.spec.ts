// Home against the real doze-aws: its values are what `/api/v1/about`, `/api/v1/services` and `/api/v1/home` answer (the
// facts of the instance are on Settings now; the refresh test reads them there).
import { about, CUT, expect, open, services, test } from "./fixtures";

test("Home says what this doze-aws is, as /about says it", async ({ page }) => {
  await open(page, "/", "Home");
  const a = await about(page);
  expect(a.account).toBe("111122223333");
  await expect(page.getByTestId("home-sub")).toContainText(`account ${a.account}`);
  await expect(page.getByTestId("home-sub")).toContainText(a.region);
  await expect(page.getByTestId("endpoint")).toHaveText(a.endpoint);
  await expect(page.getByTestId("endpoint-env")).toHaveText(`AWS_ENDPOINT_URL=${a.endpoint}`);
});

test("Settings lists every service of the build, with its state and how much of it is built", async ({ page }) => {
  await open(page, "/settings", "Settings");
  const { services: list } = await services(page);
  expect(list.length).toBeGreaterThan(10);
  const table = page.getByRole("table", { name: "Services of this build" });
  const body = table.locator("tbody tr[data-row]");
  await expect(body).toHaveCount(list.length);
  for (const s of list) {
    const row = table.locator(`tr[data-row="${s.name}"]`);
    await expect(row.locator("td").nth(0)).toHaveText(s.title);
    await expect(row.locator("td").nth(1)).toHaveText(s.name);
    await expect(row.locator("td").nth(2)).toHaveText(s.served ? "Served" : "Not served");
    await expect(row.locator("td").nth(4)).toHaveText(`${s.built.toLocaleString("en-US")} of ${s.total.toLocaleString("en-US")}`);
    await expect(row.locator("td").nth(6)).toHaveText(s.apis.map((x) => x.sdk_id).join(", "));
  }
  const served = list.filter((s) => s.served).length;
  await expect(page.getByTestId("services-counts")).toContainText(`${served} of ${list.length} are served in this run`);
  // Sorting by what is built puts the largest first.
  const built = table.getByRole("columnheader", { name: "Operations built" });
  await built.getByRole("button").click();
  await expect(built).toHaveAttribute("aria-sort", "descending");
  const most = [...list].sort((x, y) => y.built - x.built)[0]!;
  await expect(body.first().locator("td").nth(0)).toHaveText(most.title);
});

test("Refresh fetches again: a region first called after the page was drawn appears", async ({ page, cli, browserName }) => {
  // A region no other browser's run of this test uses, so it is new to this doze-aws now.
  const region = { firefox: "ap-south-1", webkit: "ca-central-1", chromium: "sa-east-1" }[browserName] ?? "eu-north-1";
  await open(page, "/settings", "Settings");
  await expect(page.getByTestId("fact-region")).not.toContainText(region);
  await expect(page.getByTestId("refresh-stamp")).toContainText("Updated");
  expect(cli(["sqs", "list-queues"], { region }).code).toBe(0);
  // Nothing changes by itself: the page is a snapshot until it is refreshed.
  await page.waitForTimeout(600);
  await expect(page.getByTestId("fact-region")).not.toContainText(region);
  await page.getByTestId("refresh-now").click();
  await expect(page.getByTestId("fact-region")).toContainText(region);
});

test.describe("a fetch that fails", () => {
  test.use({ allowErrors: CUT });

  test("says what could not be loaded and what to do, and Try again loads it", async ({ page }) => {
    await page.route("**/api/v1/services", (route) => route.abort("connectionrefused"));
    await open(page, "/", "Home");
    const block = page.getByTestId("services-error");
    await expect(block).toBeVisible({ timeout: 15_000 });
    await expect(block).toHaveAttribute("role", "alert");
    await expect(block).toContainText("The services could not be loaded");
    await expect(block).toContainText("doze-aws did not answer. Is it still running?");
    await expect(block).toContainText("Check that doze-aws is still running, then try again.");
    await expect(block).toContainText("UNREACHABLE");
    // The rest of the page is not blank: what did load is shown.
    await expect(page.getByTestId("endpoint")).toBeVisible();
    await page.unroute("**/api/v1/services");
    await page.getByTestId("services-error-retry").click();
    await expect(page.getByTestId("awake-group").first()).toBeVisible();
    await expect(block).toHaveCount(0);
  });

  test("a refusal is shown in the server's own words, with its code", async ({ page, refusals }) => {
    // The test makes the server refuse.
    refusals.allow(/403 GET .*\/api\/v1\/about$/);
    // The request is sent on without the proof header, so the real server refuses it: the words on the page are its own.
    await page.route("**/api/v1/about", (route) => {
      const headers = { ...route.request().headers() };
      delete headers["x-console"];
      return route.continue({ headers });
    });
    await open(page, "/", "Home");
    const block = page.getByTestId("about-error");
    await expect(block).toBeVisible({ timeout: 15_000 });
    await expect(block).toContainText("What this doze-aws is could not be loaded");
    await expect(block).toContainText("answers only its own pages");
    await expect(block).toContainText("FORBIDDEN · HTTP 403");
    // No chip is made up while the server's answer is not known.
    await expect(page.getByTestId("chip-endpoint")).toHaveCount(0);
  });
});
