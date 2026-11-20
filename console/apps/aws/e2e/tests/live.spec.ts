// Live against the real doze-aws: calls made with the real AWS CLI arrive on the page without a reload, Pause holds them,
// a row opens its drawer, and a doze-aws that stops is said and survived.
import { aws, start } from "../server";
import { lines } from "../../src/pages/live/repeats";
import { LISTEN_PORT } from "../global-setup";
import { about, calls, CUT, expect, open, openLive, rows, test, unique } from "./fixtures";

test("calls made with the AWS CLI appear as they are answered, without a reload", async ({ page, cli, accessKey, browserName }) => {
  await openLive(page);
  // The page that is open is the one that must show them: a reload would lose this mark.
  await page.evaluate(() => ((window as unknown as { mark: string }).mark = "same page"));
  const queue = unique("orders", browserName);

  expect(cli(["sqs", "create-queue", "--queue-name", queue]).code).toBe(0);
  const first = rows(page).first();
  await expect(first.locator("td").nth(2)).toHaveText("CreateQueue");
  await expect(first.locator("td").nth(1)).toHaveText("SQS");
  await expect(first.locator("td").nth(4)).toHaveText(accessKey);
  await expect(first.locator('[data-slot="result"]')).toHaveAttribute("data-outcome", "ok");
  await expect(first.locator('[data-slot="result"]')).toContainText("200");

  // A call that fails: the queue does not exist. The code is the one the CLI reports (SQS answers its query-compatible code in
  // x-amzn-query-error, as AWS does: conformance/snapshots/test_sqs.json).
  expect(cli(["sqs", "get-queue-url", "--queue-name", `${queue}-missing`]).code).not.toBe(0);
  await expect(first.locator("td").nth(2)).toHaveText("GetQueueUrl");
  await expect(first.locator('[data-slot="result"]')).toHaveAttribute("data-outcome", "error");
  await expect(first.locator('[data-slot="result"]')).toHaveText(/ERROR\s*400\s*AWS\.SimpleQueueService\.NonExistentQueue/);

  // An operation this build does not have: not built, not an error of the caller's.
  expect(cli(["ssm", "describe-instance-information"]).code).not.toBe(0);
  await expect(first.locator("td").nth(2)).toHaveText("DescribeInstanceInformation");
  await expect(first.locator('[data-slot="result"]')).toHaveAttribute("data-outcome", "not-built");
  await expect(first.locator('[data-slot="result"]')).toHaveText(/NOT BUILT\s*501\s*NotImplemented/);

  // S3 names what was acted on.
  const bucket = unique("shots", browserName);
  expect(cli(["s3api", "create-bucket", "--bucket", bucket]).code).toBe(0);
  expect(cli(["s3api", "put-object", "--bucket", bucket, "--key", "notes/hello.txt", "--body", import.meta.filename]).code).toBe(0);
  await expect(first.locator("td").nth(2)).toHaveText("PutObject");
  await expect(first.locator("td").nth(3)).toHaveText(`${bucket}/notes/hello.txt`);

  // What is on the page is what the record holds, newest first, and the figures are the record's own.
  const record = await calls(page);
  const newest = [...record.calls].reverse();
  // One row a call, but calls in a row that repeat each other are one row (repeats.ts); this console's own calls (the Resources
  // tests' in another browser) are hidden until asked for.
  const shown = newest.slice(0, record.ring.capacity).filter((c) => c.caller !== "console");
  await expect(rows(page)).toHaveCount(lines(shown, new Set()).length);
  await expect(rows(page).first()).toHaveAttribute("data-row", String(shown[0]!.seq));
  await expect(page.getByTestId("live-recorded")).toContainText(record.ring.recorded.toLocaleString("en-US"));
  await expect(page.getByTestId("live-held")).toContainText(`${record.ring.held.toLocaleString("en-US")} of ${record.ring.capacity.toLocaleString("en-US")}`);
  expect(await page.evaluate(() => (window as unknown as { mark?: string }).mark)).toBe("same page");
  // Every column fits a 1440 px window: the table does not scroll sideways in its box, and no time is cut.
  expect(await page.locator('[data-slot="data-table"]').evaluate((el) => Math.max(0, el.scrollWidth - el.clientWidth))).toBe(0);
  expect(await first.locator("td").nth(0).evaluate((td) => Math.max(0, td.scrollWidth - td.clientWidth))).toBe(0);
  expect(await first.locator("td").nth(6).evaluate((td) => Math.max(0, td.scrollWidth - td.clientWidth))).toBe(0);
});

test("Pause holds new calls aside and counts them; Resume shows them", async ({ page, cli }) => {
  await openLive(page);
  expect(cli(["sqs", "list-queues"]).code).toBe(0);
  await expect(rows(page).first().locator("td").nth(2)).toHaveText("ListQueues");
  const top = await rows(page).first().getAttribute("data-row");
  const before = await rows(page).count();

  await page.getByTestId("live-pause").click();
  await expect(page.getByTestId("live-status")).toHaveText("Paused");
  expect(cli(["sns", "list-topics"]).code).toBe(0);
  expect(cli(["dynamodb", "list-tables"]).code).toBe(0);
  await expect(page.getByTestId("held-pill")).toContainText("2 new");
  // Nothing on screen moved.
  await expect(rows(page).first()).toHaveAttribute("data-row", top!);
  await expect(rows(page)).toHaveCount(before);

  await page.getByTestId("live-resume").click();
  await expect(page.getByTestId("live-status")).toHaveText("Live");
  await expect(page.getByTestId("held-pill")).toHaveCount(0);
  await expect(rows(page)).toHaveCount(before + 2);
  await expect(rows(page).nth(0).locator("td").nth(2)).toHaveText("ListTables");
  await expect(rows(page).nth(1).locator("td").nth(2)).toHaveText("ListTopics");
  // The pill shows them too.
  await page.getByTestId("live-pause").click();
  expect(cli(["sqs", "list-queues"]).code).toBe(0);
  await page.getByTestId("held-pill").click();
  await expect(page.getByTestId("live-status")).toHaveText("Live");
  await expect(rows(page)).toHaveCount(before + 3);
});

test("a row opens a drawer over the page with everything the record has for the call", async ({ page, cli, accessKey, browserName }) => {
  expect(cli(["sqs", "get-queue-url", "--queue-name", unique("nope", browserName)], { region: "eu-west-1" }).code).not.toBe(0);
  await openLive(page);
  const record = (await calls(page)).calls.filter((c) => c.caller === accessKey && c.action === "GetQueueUrl").pop()!;
  expect(record.outcome).toBe("error");

  await page.locator(`tr[data-row="${record.seq}"]`).click();
  const sheet = page.getByTestId("call-sheet");
  await expect(sheet).toBeVisible();
  await expect(sheet).toHaveAttribute("role", "dialog");
  // An overlay, not a page: Live is still there behind it, and the address names the call.
  await expect(page).toHaveURL(new RegExp(`/live\\?call=${record.seq}$`));
  await expect(page.locator("h1").first()).toHaveText("Live");
  await expect(rows(page).first()).toBeAttached();

  await expect(page.getByTestId("call-title")).toHaveText("GetQueueUrl");
  await expect(page.getByTestId("call-summary")).toContainText("ERROR");
  await expect(page.getByTestId("call-summary")).toContainText("the caller's to fix");
  const at = new Date(record.at_ms).toISOString();
  await expect(page.getByTestId("call-at")).toHaveText(`${at.slice(0, 10)} ${at.slice(11, 23)} UTC`);
  await expect(page.getByTestId("call-duration")).toContainText(`${record.duration_us.toLocaleString("en-US")} µs`);
  await expect(page.getByTestId("call-service")).toContainText("sqs");
  await expect(page.getByTestId("call-api")).toContainText("SQS");
  await expect(page.getByTestId("call-action")).toHaveText("GetQueueUrl");
  // What it acted on: the queue it asked for, by the member of its input that names it.
  await expect(page.getByTestId("call-resource")).toContainText(record.resource!);
  await expect(page.getByTestId("call-resource")).toContainText("The call's QueueName.");
  await expect(page.getByTestId("call-caller")).toContainText(accessKey);
  await expect(page.getByTestId("call-region")).toHaveText("eu-west-1");
  await expect(page.getByTestId("call-status")).toHaveText("400");
  await expect(page.getByTestId("call-error-code")).toContainText("AWS.SimpleQueueService.NonExistentQueue");
  await expect(page.getByTestId("call-summary").locator('[data-slot="result"]')).toHaveAttribute("data-outcome", "error");
  await expect(page.getByTestId("call-outcome")).toHaveText("error");
  await expect(page.getByTestId("call-request-id")).toContainText(record.request_id);
  await expect(page.getByTestId("call-seq")).toContainText(String(record.seq));

  // Esc closes it; the address is Live's again.
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await expect(page).toHaveURL(/\/live$/);
  // The address alone opens it (a link someone was sent, a reload).
  await openLive(page, `/live?call=${record.seq}`);
  await expect(page.getByTestId("call-request-id")).toContainText(record.request_id);
  await sheet.getByRole("button", { name: "Close" }).click();
  await expect(sheet).toHaveCount(0);
});

test("the drawer opens from the keyboard, and a call the record no longer holds says so", async ({ page, cli }) => {
  expect(cli(["sts", "get-caller-identity"]).code).toBe(0);
  await openLive(page);
  const button = rows(page).first().getByTestId("open-call");
  await button.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("call-sheet")).toBeVisible();
  await expect(page.getByTestId("call-title")).toHaveText("GetCallerIdentity");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("call-sheet")).toHaveCount(0);
  // The focus is back on what opened it.
  await expect(button).toBeFocused();

  await openLive(page, "/live?call=99999999");
  await expect(page.getByTestId("call-gone")).toContainText("no longer in the record");
  // An address that is not a call's number opens nothing.
  await openLive(page, "/live?call=abc");
  await expect(page.getByTestId("call-sheet")).toHaveCount(0);
});

test.describe("when requests fail", () => {
  test.use({ allowErrors: CUT });

  test("calls that cannot be loaded are said, with what to do, and Try again loads them", async ({ page }) => {
    await page.route("**/api/v1/calls*", (route) => route.abort("connectionrefused"));
    await open(page, "/live", "Live");
    const block = page.getByTestId("live-error");
    await expect(block).toBeVisible();
    await expect(block).toContainText("The calls could not be loaded");
    await expect(block).toContainText("doze-aws did not answer. Is it still running?");
    await expect(block).toContainText("If it stopped, start it");
    await expect(page.getByTestId("live-status")).toHaveText("Reconnecting");
    await page.unroute("**/api/v1/calls*");
    await page.getByTestId("live-error-retry").click();
    await expect(page.getByTestId("live-status")).toHaveText("Live");
    await expect(block).toHaveCount(0);
  });

  test("a doze-aws that stops is said on the page, and the page comes back by itself when it starts again", async ({ page, browserName }) => {
    // A doze-aws of this test's own (it is stopped in the middle), on ports of its own per browser.
    const at = { firefox: 0, webkit: 2, chromium: 4 }[browserName] ?? 6;
    let server = await start(LISTEN_PORT + 10 + at, LISTEN_PORT + 11 + at);
    const { endpoint, console: consoleUrl } = server;
    try {
      expect(aws(endpoint, ["sqs", "create-queue", "--queue-name", "before-the-stop"]).code).toBe(0);
      await page.goto(`${consoleUrl}/live`);
      await expect(page.getByTestId("live-status")).toHaveText("Live");
      await expect(rows(page).first().locator("td").nth(2)).toHaveText("CreateQueue");
      const firstRun = (await about(page, consoleUrl)).pid;

      await server.stop();
      const stopped = page.getByTestId("live-stopped");
      await expect(stopped).toBeVisible({ timeout: 15_000 });
      await expect(stopped).toContainText("The live stream stopped");
      await expect(page.getByTestId("live-status")).toHaveText("Reconnecting");
      // What was on the page stays: it is not blanked by the failure.
      await expect(rows(page).first().locator("td").nth(2)).toHaveText("CreateQueue");

      server = await start(LISTEN_PORT + 10 + at, LISTEN_PORT + 11 + at);
      expect((await about(page, consoleUrl)).pid).not.toBe(firstRun);
      await expect(page.getByTestId("live-status")).toHaveText("Live", { timeout: 20_000 });
      await expect(stopped).toHaveCount(0);
      // The record is the new run's: the old run's call is gone from the page, and a new call arrives.
      await expect(rows(page)).toHaveCount(0);
      await expect(page.getByText("No calls yet")).toBeVisible();
      expect(aws(endpoint, ["sqs", "list-queues"]).code).toBe(0);
      await expect(rows(page)).toHaveCount(1);
      await expect(rows(page).first().locator("td").nth(2)).toHaveText("ListQueues");
    } finally {
      // Each start had a data directory of its own, which its stop removes.
      await server.stop();
    }
  });
});
