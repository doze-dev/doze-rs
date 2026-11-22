// The EventBridge boards of the archives, the replays, and the API destinations with their connections, against a real doze-aws seeded as the boards' "larder"
// world. The empty lists come first (nothing has been replayed or made yet); the archives and the destinations are emptied last. In Chromium each board is
// photographed light and dark into SHOTS_DIR.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { putEvent, seedLarder } from "./eventbridge-seed";
import { expect, expectNoErrorAnswers, test } from "./fixtures";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-eventbridge");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45458);
const at = (b: string) => BASE + ({ firefox: 0, webkit: 10, chromium: 20 }[b] ?? 30);

test.describe.configure({ mode: "serial" });

let server: Running;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

async function shot(page: Page, board: string) {
  if (browserKind !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
  const height = await page.evaluate(() => Math.max(document.documentElement.scrollHeight, window.innerHeight));
  await page.setViewportSize({ width: 1440, height });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(SHOTS, `${board}.light.png`) });
  await page.evaluate(() => document.documentElement.classList.add("dark"));
  await page.waitForTimeout(250);
  await page.screenshot({ path: join(SHOTS, `${board}.dark.png`) });
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await page.setViewportSize({ width: 1440, height: 900 });
}

const go = async (page: Page, path: string, title: string | RegExp) => {
  await page.goto(`${server.console}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
};
const submit = (page: Page) => page.getByTestId("form-submit").click();

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  seedLarder(server.endpoint, server.account);
  for (let i = 0; i < 3; i++) expect(putEvent(server.endpoint, "larder", "larder.payments", "PaymentSettled", { order_id: `o-${i}` }).code).toBe(0);
  expect(putEvent(server.endpoint, "larder", "larder.orders", "OrderPlaced", { order_id: "o-1", customer_id: "c-1", total_cents: 4850 }).code).toBe(0);
});
test.afterAll(async () => {
  await server.stop();
});

test("eventbridge__replays-none: before any replay was started the list says what a replay is", async ({ page }) => {
  await go(page, "/resources/eventbridge/replays", "Replays");
  await expect(page.getByTestId("list-empty")).toContainText("No replays");
  await expect(page.getByTestId("list-empty")).toContainText("A replay sends archived events back to a bus. Start one from an archive.");
  await expect(page.getByTestId("strip-replays")).toHaveAttribute("aria-current", "page");
  await shot(page, "eventbridge__replays-none");
});

test("eventbridge__api-destinations-none: before any API destination was made the list offers to make one", async ({ page }) => {
  await go(page, "/resources/eventbridge/api-destinations", "API destinations");
  await expect(page.getByTestId("list-empty")).toContainText("No API destinations");
  await expect(page.getByTestId("list-empty")).toContainText("An API destination is an HTTP endpoint a rule can call, with the connection that holds its credentials.");
  await shot(page, "eventbridge__api-destinations-none");
});

test("eventbridge__archives-list: events and size per archive, retention in days, rows chosen for Replay and Delete", async ({ page }) => {
  await go(page, "/resources/eventbridge/archives", "Archives");
  await expect(page.getByTestId("list-sentence")).toContainText("3 archives hold 8 events from larder.");
  await expect(page.getByTestId("list-sentence")).toContainText("payments-90d keeps only larder.payments events, for 90 days.");
  const table = page.getByRole("table", { name: "Archives" });
  await expect(table.locator("tr[data-row]")).toHaveCount(3);
  await expect(table.locator("tr[data-row]", { hasText: "payments-90d" })).toContainText("90 days");
  await expect(table.locator("tr[data-row]", { hasText: "payments-90d" })).toContainText("3");
  await expect(page.getByText("retention is applied", { exact: false }).first()).toBeVisible();
  await expect(page.getByTestId("action-replay")).toBeDisabled();
  await page.getByTestId("select-payments-90d").click();
  await expect(page.getByTestId("action-replay")).toBeEnabled();
  await shot(page, "eventbridge__archives-list");
});

test("eventbridge__archive-create: a name, the bus, indefinite or days, a pattern, and the call it will make", async ({ page }) => {
  await go(page, "/resources/eventbridge/buses/larder", "larder");
  await page.getByTestId("archive-create").click();
  await expect(page.locator("h1").first()).toHaveText("Create archive");
  await page.getByTestId("archive-name").fill("orders-7d");
  await expect(page.getByTestId("field-source")).toContainText("larder already has larder-all, orders-30d and payments-90d.");
  await expect(page.getByTestId("call-panel-code")).not.toContainText("RetentionDays");
  await page.getByTestId("retention-days").click();
  await page.getByTestId("archive-days").fill("7");
  await page.getByTestId("filter-filter").click();
  await page.getByTestId("archive-pattern").fill('{\n  "source": ["larder.orders"]\n}');
  const call = page.getByTestId("call-panel-code");
  await expect(call).toContainText('"RetentionDays": 7');
  await expect(call).toContainText("larder.orders");
  await expect(page.getByTestId("field-retention")).toContainText("Applied here:");
  await shot(page, "eventbridge__archive-create");
  await page.getByTestId("archive-days").fill("0");
  await submit(page);
  await expect(page.getByTestId("field-retention")).toContainText("Retention is a whole number of days, 1 or more.");
  await page.getByTestId("archive-days").fill("7");
  await submit(page);
  await expect(page.locator("h1").first()).toHaveText("orders-7d");
  expect(cli(["events", "describe-archive", "--archive-name", "orders-7d"])).toContain('"RetentionDays": 7');
});

test("eventbridge__replay-start: an archive, the bus it goes back to, the rules that receive it and a window of event time", async ({ page }) => {
  await go(page, "/resources/eventbridge/archives", "Archives");
  await page.getByTestId("select-payments-90d").click();
  await page.getByTestId("action-replay").click();
  await expect(page.locator("h1").first()).toHaveText("Start new replay");
  await expect(page.getByTestId("replay-destination")).toHaveValue("larder");
  await expect(page.getByTestId("field-archive")).toContainText("payments-90d holds 3 events.");
  await page.getByTestId("replay-name").fill("payments-redo");
  await page.getByTestId("replay-description").fill("Re-run the settled payments");
  await page.getByTestId("rules-some").click();
  await expect(page.getByTestId("rule-list")).toContainText("payment-settled");
  await expect(page.getByTestId("rule-list")).toContainText("order-placed");
  await page.getByTestId("rule-payment-settled").click();
  const call = page.getByTestId("call-panel-code");
  await expect(call).toContainText("StartReplay");
  await expect(call).toContainText(`arn:aws:events:us-east-1:${server.account}:rule/larder/payment-settled`);
  await expect(call).toContainText(`arn:aws:events:us-east-1:${server.account}:archive/payments-90d`);
  await shot(page, "eventbridge__replay-start");
  await submit(page);
  await expect(page.locator("h1").first()).toHaveText("payments-redo");
  await expect.poll(() => cli(["events", "describe-replay", "--replay-name", "payments-redo"]), { timeout: 15_000 }).toContain("COMPLETED");
  // The same name is not started twice, and the archive keeps its events.
  expect(cli(["events", "describe-archive", "--archive-name", "payments-90d"])).toContain('"EventCount": 3');
});

test("eventbridge__replays-list: the window, the last event replayed and when it started", async ({ page }) => {
  await go(page, "/resources/eventbridge/replays", "Replays");
  const table = page.getByRole("table", { name: "Replays" });
  await expect(table.locator("tr[data-row]")).toHaveCount(1);
  await expect(table).toContainText("payments-redo");
  await expect(table).toContainText("payments-90d");
  await expect(table).toContainText("Completed");
  await expect(page.getByTestId("list-sentence")).toContainText("1 replay, completed.");
  await expect(page.getByText("can be cancelled while it is Starting or Running")).toBeVisible();
  await shot(page, "eventbridge__replays-list");
});

test("eventbridge__connection-create and api-destination-create: a connection made on the way, chosen on return, and the destination it signs in", async ({ page }) => {
  await go(page, "/resources/eventbridge/api-destinations?create=yes", "Create API destination");
  await page.getByTestId("dest-name").fill("status-page");
  await page.getByTestId("dest-endpoint").fill("https://status.larder.example/v1/incidents/*");
  await page.getByTestId("conn-new").click();
  await expect(page.locator("h1").first()).toHaveText("Create connection");
  await page.getByTestId("conn-name").fill("status-key");
  await page.getByTestId("conn-description").fill("API key for the status page");
  await page.getByTestId("conn-key-value").fill("s3cret-value");
  await page.getByTestId("add-param").click();
  await page.getByTestId("param-key").fill("X-Source");
  await page.getByTestId("param-value").fill("larder");
  const call = page.getByTestId("call-panel-code");
  await expect(call).toContainText("CreateConnection");
  await expect(call).toContainText('"AuthorizationType": "API_KEY"');
  await expect(call).toContainText('"ApiKeyName": "X-Api-Key"');
  await expect(call).toContainText('"Key": "X-Source"');
  await expect(page.getByTestId("section-detail")).toContainText("Not available in doze-aws: private APIs need VPC Lattice.");
  await shot(page, "eventbridge__connection-create");
  await submit(page);
  // Back on the destination, what was typed kept and the new connection chosen.
  await expect(page.locator("h1").first()).toHaveText("Create API destination");
  await expect(page.getByTestId("dest-name")).toHaveValue("status-page");
  await expect(page.getByTestId("dest-endpoint")).toHaveValue("https://status.larder.example/v1/incidents/*");
  await expect(page.getByTestId("connection-state")).toContainText("status-key is Authorized.");
  await page.getByTestId("dest-description").fill("Post incident updates");
  await page.getByTestId("dest-rate").fill("300");
  await expect(page.getByTestId("call-panel-code")).toContainText("CreateApiDestination");
  await expect(page.getByTestId("call-panel-code")).toContainText('"InvocationRateLimitPerSecond": 300');
  await expect(page.getByTestId("section-detail")).toContainText("Stored, not applied locally.");
  await shot(page, "eventbridge__api-destination-create");
  await submit(page);
  await expect(page.locator("h1").first()).toHaveText("status-page");
  expect(cli(["events", "describe-api-destination", "--name", "status-page"])).toContain("ACTIVE");
  // A secret is never read back.
  expect(cli(["events", "describe-connection", "--name", "status-key"])).not.toContain("s3cret-value");
});

test("eventbridge__api-destinations-list: the endpoint, the method, the connection and the rate limit of each", async ({ page }) => {
  cli(["events", "create-connection", "--name", "ops-basic", "--authorization-type", "BASIC", "--auth-parameters", JSON.stringify({ BasicAuthParameters: { Username: "ops", Password: "pw" } })]);
  const arn = (JSON.parse(cli(["events", "describe-connection", "--name", "ops-basic"])) as { ConnectionArn: string }).ConnectionArn;
  cli(["events", "create-api-destination", "--name", "ops-notify", "--connection-arn", arn, "--invocation-endpoint", "https://ops.larder.example/notify", "--http-method", "POST", "--invocation-rate-limit-per-second", "50"]);
  await go(page, "/resources/eventbridge/api-destinations", "API destinations");
  const table = page.getByRole("table", { name: "API destinations" });
  await expect(table.locator("tr[data-row]")).toHaveCount(2);
  await expect(table.locator("tr[data-row]", { hasText: "status-page" })).toContainText("https://status.larder.example/v1/incidents/*");
  await expect(table.locator("tr[data-row]", { hasText: "status-page" })).toContainText("status-key");
  await expect(table.locator("tr[data-row]", { hasText: "ops-notify" })).toContainText("50");
  await expect(page.getByTestId("list-sentence")).toContainText("2 API destinations, both active. ops-notify signs in through the ops-basic connection.");
  await expect(page.getByText("EventBridge gives the endpoint 5 seconds to answer.")).toBeVisible();
  await shot(page, "eventbridge__api-destinations-list");
});

test("eventbridge__archives-none: the last archive deleted leaves the empty state, and the events go with it", async ({ page, refusals }) => {
  await go(page, "/resources/eventbridge/archives", "Archives");
  for (const name of ["larder-all", "orders-30d", "orders-7d", "payments-90d"]) {
    await page.getByTestId(`select-${name}`).click();
    await page.getByTestId("action-delete").click();
    await expect(page.getByTestId("archive-delete")).toContainText(`Delete ${name}`);
    await page.getByTestId("archive-delete-submit").click();
    await expect(page.getByTestId("deleted-banner")).toContainText(`Deleted ${name}`);
    await expectNoErrorAnswers(page, refusals);
  }
  await expect(page.getByTestId("list-empty")).toContainText("No archives");
  await expect(page.getByTestId("list-empty")).toContainText("An archive stores the events that match a pattern so you can replay them later.");
  await expect(page.getByTestId("list-empty").getByRole("button", { name: "Create archive" })).toBeVisible();
  await shot(page, "eventbridge__archives-none");
  expect(cli(["events", "list-archives"])).not.toContain("larder-all");
});
