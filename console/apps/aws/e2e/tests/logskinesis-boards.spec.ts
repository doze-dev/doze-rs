// The 39 CloudWatch Logs and Kinesis boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder"
// world with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board. The flows run in the
// order the boards tell them: a group is made, searched, given a metric filter and a subscription, and deleted; a stream is made, edited, read and deleted.
// The groups and the streams are deleted last, which leaves the empty lists.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { CUT, expect, test } from "./fixtures";
import { seedLarder } from "./logskinesis-seed";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-logskinesis");
const BASE = 45000;
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });
// A few boards are the service's refusal (a name in use): the browser logs each 400 as a failed load.
test.use({ allowErrors: CUT });

let server: Running;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};
const json = <T>(args: string[]) => JSON.parse(cli(args)) as T;

/** Photographs the page as the board is: light, then dark (the class the theme switch sets), in Chromium only. */
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
const group = (name: string, q = "") => `/resources/logs/groups/${encodeURIComponent(name)}${q}`;
const stream = (name: string, g: string) => `/resources/logs/streams/${encodeURIComponent(name)}?group=${encodeURIComponent(g)}`;
const kstream = (name: string, q = "") => `/resources/kinesis/streams/${encodeURIComponent(name)}${q}`;

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  seedLarder(server.endpoint, server.account);
});
test.afterAll(async () => {
  await server.stop();
});

test("logs__log-groups and retention-edit: the list with its sentence, the retention opened from its cell", async ({ page }) => {
  await go(page, "/resources/logs/groups", "Log groups");
  await expect(page.getByTestId("list-sentence")).toContainText("6 log groups. /larder/api has 2 metric filters and keeps events for 30 days. Choose a retention in the Retention column to change it, or the number in the Metric filters column to see a group's filters.");
  const row = (name: string) => page.locator('[data-slot="data-table"] tbody tr[data-row]').filter({ hasText: name });
  await expect(row("/larder/api")).toContainText("30 days");
  await expect(row("/larder/api")).toContainText("Standard");
  await expect(row("/larder/scratch")).toContainText("Never Expire");
  await expect(page.getByTestId("list-teaching")).toContainText("DescribeLogGroups returns");
  await expect(page.getByTestId("action-delete")).toBeDisabled();
  await shot(page, "logs__log-groups");
  // The Retention cell opens the setting.
  await row("/larder/api").getByTestId("retention-open").click();
  await expect(page.getByTestId("retention-dialog")).toContainText("Expire events after");
  await expect(page.getByTestId("retention-dialog-call")).toContainText("PutRetentionPolicy");
  await shot(page, "logs__retention-edit");
  await page.getByTestId("retention-select").click();
  await page.getByRole("option", { name: "60 days" }).click();
  await page.getByTestId("retention-dialog-save").click();
  await expect(row("/larder/api")).toContainText("60 days");
  expect(json<{ logGroups: { retentionInDays: number }[] }>(["logs", "describe-log-groups", "--log-group-name-prefix", "/larder/api"]).logGroups[0]?.retentionInDays).toBe(60);
  // Never Expire removes the policy.
  await row("/larder/api").getByTestId("retention-open").click();
  await page.getByTestId("retention-select").click();
  await page.getByRole("option", { name: "Never Expire" }).click();
  await expect(page.getByTestId("retention-dialog-call")).toContainText("DeleteRetentionPolicy");
  await page.getByTestId("retention-dialog-save").click();
  await expect(row("/larder/api")).toContainText("Never Expire");
  cli(["logs", "put-retention-policy", "--log-group-name", "/larder/api", "--retention-in-days", "30"]);
});

test("logs__log-group-create and log-group-created: the form, its call, and the empty group it lands on", async ({ page }) => {
  await go(page, "/resources/logs/groups?create=yes", "Create log group");
  await page.getByTestId("group-name").fill("/larder/notifications");
  await expect(page.getByTestId("log-group-create")).toContainText("The other groups here are named /aws/lambda/<function> or /larder/<app>.");
  await expect(page.getByTestId("call-panel")).toContainText('"logGroupClass": "STANDARD"');
  await expect(page.getByTestId("call-panel")).toContainText("/larder/notifications");
  await expect(page.getByTestId("log-group-create")).toContainText("Stored, not applied locally.");
  await shot(page, "logs__log-group-create");
  // A name that is not allowed is said on the field.
  await page.getByTestId("group-name").fill("aws/own");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("log-group-create")).toContainText("A name cannot begin with aws/");
  await page.getByTestId("group-name").fill("/larder/notifications");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("/larder/notifications");
  await expect(page.getByTestId("created-banner")).toContainText("CreateLogGroup returned 200");
  await expect(page.getByTestId("detail-sentence")).toContainText("/larder/notifications is empty. Nothing has written to it yet.");
  await expect(page.getByTestId("streams-empty")).toContainText("DescribeLogStreams returned no log streams");
  await shot(page, "logs__log-group-created");
  expect(json<{ logGroups: unknown[] }>(["logs", "describe-log-groups", "--log-group-name-prefix", "/larder/notifications"]).logGroups).toHaveLength(1);
});

test("logs__log-group-detail, log-streams-none, metric-filters: a group's page, its tabs and the empty tab of a group with nothing", async ({ page }) => {
  await go(page, group("/larder/api"), "/larder/api");
  await expect(page.getByTestId("detail-badge")).toHaveText("Standard");
  await expect(page.getByTestId("detail-sentence")).toContainText("/larder/api took 10 events in the last hour from 5 streams. The newest event arrived at");
  await expect(page.getByTestId("tab-streams")).toContainText("5");
  await expect(page.getByTestId("tab-metric-filters")).toContainText("2");
  await expect(page.getByTestId("details")).toContainText("arn:aws:logs:us-east-1:");
  await expect(page.getByTestId("wired-strip")).toContainText("metric Larder/Api ApiErrors");
  await expect(page.getByTestId("recent-calls")).toContainText("PutLogEvents");
  await expect(page.getByTestId("verb-search")).toHaveText("Search log group");
  await page.getByTestId("actions-menu").click();
  await expect(page.getByRole("menuitem", { name: "Create metric filter" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Subscription filters (Lambda)" })).toBeVisible();
  await page.keyboard.press("Escape");
  await shot(page, "logs__log-group-detail");
  await go(page, group("/larder/worker"), "/larder/worker");
  await expect(page.getByTestId("streams-empty")).toContainText("Nothing has written to /larder/worker yet");
  await shot(page, "logs__log-streams-none");
  await go(page, group("/larder/worker", "?tab=metric-filters"), "/larder/worker");
  await expect(page.getByTestId("filters-empty")).toContainText("No metric filter turns events in /larder/worker into a metric.");
  await shot(page, "logs__metric-filters-none");
  await go(page, group("/larder/api", "?tab=metric-filters"), "/larder/api");
  const rows = page.getByTestId("group-filters").locator('[data-slot="data-table"] tbody tr[data-row]');
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText("api-errors");
  await expect(rows.first()).toContainText("Larder/Api");
  await expect(page.getByTestId("group-filters")).toContainText("The columns are fields that DescribeMetricFilters returns");
  await shot(page, "logs__metric-filters");
});

test("logs__log-stream-events and events-none: a stream's events over a range, a pattern, a zone", async ({ page }) => {
  await go(page, stream("payments-8c2e1", "/larder/api"), "payments-8c2e1");
  await expect(page.getByTestId("log-event")).toHaveCount(5);
  await expect(page.getByTestId("stream-span")).toContainText("5 events,");
  await page.getByTestId("log-event-toggle").nth(2).click();
  await expect(page.getByTestId("log-event-open")).toContainText('"elapsed_ms": 10004');
  await expect(page.getByTestId("stream-viewer")).toContainText("a filter pattern");
  await shot(page, "logs__log-stream-events");
  await page.getByTestId("zone-local").click();
  await expect(page.getByTestId("zone-local")).toHaveAttribute("data-state", "on");
  await page.getByTestId("zone-utc").click();
  // A pattern narrows it; one with no match says so and offers a wider range.
  await page.getByTestId("stream-filter").fill('{ $.level = "error" }');
  await page.getByTestId("stream-search").click();
  await expect(page.getByTestId("log-event")).toHaveCount(1);
  await page.getByTestId("stream-filter").fill("nothing-like-this");
  await page.getByTestId("stream-search").click();
  await expect(page.getByTestId("stream-empty")).toContainText("No events match this time range or filter pattern. Try a wider range.");
  await shot(page, "logs__log-stream-events-none");
  // An absolute range.
  await page.getByTestId("range-mode-absolute").click();
  await expect(page.getByTestId("range-absolute")).toBeVisible();
});

test("logs__log-group-search: every stream for a pattern, and the call it makes", async ({ page }) => {
  await go(page, group("/larder/api", "?view=search"), "Search log group");
  await page.getByTestId("search-pattern").fill('{ $.level = "error" }');
  await expect(page.getByTestId("call-panel")).toContainText("FilterLogEvents");
  await page.getByTestId("search-run").click();
  await expect(page.getByTestId("search-count")).toContainText("2 events in the last 1 hour match the pattern, in 2 of 5 streams.");
  await expect(page.getByTestId("search-end")).toContainText("nextToken");
  await expect(page.getByTestId("search-examples")).toContainText("both terms");
  await shot(page, "logs__log-group-search");
});

test("logs__metric-filter-create, metric-filter-assign, metric-filter-delete: test a pattern, name the metric, delete the filter", async ({ page }) => {
  await go(page, group("/larder/api", "?view=metric-filter"), "Create metric filter");
  await page.getByTestId("mf-pattern").fill('{ $.msg = "stripe timeout" }');
  await page.getByTestId("mf-test").click();
  await expect(page.getByTestId("mf-results")).toContainText("2 of 3 log event messages matched the pattern.");
  await expect(page.getByTestId("mf-results").locator("tbody tr")).toHaveCount(2);
  await expect(page.getByTestId("call-panel")).toContainText("TestMetricFilter");
  await shot(page, "logs__metric-filter-create");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Metric details");
  await page.getByTestId("mf-name").fill("stripe-timeouts");
  await page.getByTestId("mf-namespace").fill("Larder/Payments");
  await page.getByTestId("mf-metric").fill("StripeTimeouts");
  await expect(page.getByTestId("metric-filter-create")).toContainText("Larder/Payments doesn't exist yet; Larder/Images already holds ResizeErrors");
  await page.getByTestId("mf-unit").click();
  await page.getByRole("option", { name: "Count", exact: true }).click();
  await expect(page.getByTestId("call-panel")).toContainText("PutMetricFilter");
  await shot(page, "logs__metric-filter-assign");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("filter-created-banner")).toContainText("PutMetricFilter returned 200");
  const made = json<{ metricFilters: { filterName: string }[] }>(["logs", "describe-metric-filters", "--log-group-name", "/larder/api"]).metricFilters.map((f) => f.filterName);
  expect(made).toContain("stripe-timeouts");
  // Delete: the dialog names the metric that stops.
  const row = page.locator('[data-slot="data-table"] tbody tr[data-row]').filter({ hasText: "stripe-timeouts" });
  await row.getByRole("checkbox").click();
  await page.getByTestId("filter-delete").click();
  await expect(page.getByTestId("filter-delete-dialog")).toContainText("DeleteMetricFilter deletes stripe-timeouts from /larder/api. Events no longer produce StripeTimeouts in Larder/Payments.");
  await shot(page, "logs__metric-filter-delete");
  await page.getByTestId("filter-delete-dialog-submit").click();
  await expect(row).toHaveCount(0);
});

test("logs__subscription-filter-create: a Lambda destination, a pattern, a name; a function cannot subscribe to its own group", async ({ page }) => {
  await go(page, group("/larder/api", "?view=subscription"), "Subscription filter (Lambda)");
  await page.getByRole("combobox").first().click();
  await page.getByRole("option", { name: "send-email" }).click();
  await page.getByTestId("sub-pattern").fill('{ $.level = "error" }');
  await page.getByTestId("sub-name").fill("errors-to-email");
  await expect(page.getByTestId("call-panel")).toContainText("destinationArn");
  await shot(page, "logs__subscription-filter-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("subscription-created-banner")).toContainText("PutSubscriptionFilter returned 200");
  expect(json<{ subscriptionFilters: { filterName: string }[] }>(["logs", "describe-subscription-filters", "--log-group-name", "/larder/api"]).subscriptionFilters[0]?.filterName).toBe("errors-to-email");
  // Its own group: the console says no before the service is asked.
  await go(page, group("/aws/lambda/send-email", "?view=subscription"), "Subscription filter (Lambda)");
  await page.getByRole("combobox").first().click();
  await page.getByRole("option", { name: "send-email" }).click();
  await page.getByTestId("sub-name").fill("loop");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("subscription-create")).toContainText("A function can't subscribe to its own /aws/lambda/ group");
});

test("logs__log-group-delete, log-group-deleted, log-groups-none: delete from the list, then the last of them", async ({ page }) => {
  await go(page, "/resources/logs/groups", "Log groups");
  const row = (name: string) => page.locator('[data-slot="data-table"] tbody tr[data-row]').filter({ hasText: name });
  await row("/larder/scratch").getByRole("checkbox").click();
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("DeleteLogGroup deletes /larder/scratch and permanently deletes all the archived log events associated with it.");
  await shot(page, "logs__log-group-delete");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("DeleteLogGroup returned 200. /larder/scratch and the log events in it are gone.");
  await expect(row("/larder/scratch")).toHaveCount(0);
  await shot(page, "logs__log-group-deleted");
  // A protected group is the service's refusal, said in the dialog.
  cli(["logs", "create-log-group", "--log-group-name", "/larder/keep", "--deletion-protection-enabled"]);
  await page.reload();
  await row("/larder/keep").getByRole("checkbox").click();
  await page.getByTestId("action-delete").click();
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("delete-dialog-error")).toContainText("DeleteLogGroup was refused");
  await page.keyboard.press("Escape");
  const all = json<{ logGroups: { logGroupName: string }[] }>(["logs", "describe-log-groups"]).logGroups.map((g) => g.logGroupName);
  cli(["logs", "put-log-group-deletion-protection", "--log-group-identifier", "/larder/keep", "--no-deletion-protection-enabled"]);
  for (const g of all) cli(["logs", "delete-log-group", "--log-group-name", g]);
  await page.reload();
  await expect(page.getByTestId("list-empty")).toContainText("No log groups");
  await shot(page, "logs__log-groups-none");
});
