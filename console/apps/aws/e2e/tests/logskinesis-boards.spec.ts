// The 39 CloudWatch Logs and Kinesis boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder"
// world with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board. The flows run in the
// order the boards tell them: a group is made, searched, given a metric filter and a subscription, and deleted; a stream is made, edited, read and deleted.
// The groups and the streams are deleted last, which leaves the empty lists.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { expect, expectNoErrorAnswers, settled, test } from "./fixtures";
import { seedLarder } from "./logskinesis-seed";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-logskinesis");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45000);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });

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
  // The call says how many events it put.
  await expect(page.getByTestId("recent-calls").getByTestId("answer-words").filter({ hasText: /^· \d+ events?$/ }).first()).toBeVisible();
  await expect(page.getByTestId("verb-search")).toHaveText("Search log group");
  // The two creates the board draws as buttons beside Actions are buttons, and the menu has them too.
  await expect(page.getByTestId("open-metric-filter")).toHaveText("Create metric filter");
  await expect(page.getByTestId("open-subscription")).toHaveText("Subscription filters (Lambda)");
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
  // The events open in the person's own time zone, as the board has it; UTC is one click away.
  await expect(page.getByTestId("zone-local")).toHaveAttribute("data-state", "on");
  await page.getByTestId("zone-utc").click();
  await expect(page.getByTestId("zone-utc")).toHaveAttribute("data-state", "on");
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
  await expect(page.getByTestId("search-count")).toContainText("2 events in the last hour match the pattern, in 2 of 5 streams.");
  await expect(page.getByTestId("search-end")).toContainText("nextToken");
  await expect(page.getByTestId("search-examples")).toContainText("both terms");
  await shot(page, "logs__log-group-search");
});

test("logs__metric-filter-create, metric-filter-assign, metric-filter-delete: test a pattern, name the metric, delete the filter", async ({ page, refusals }) => {
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
  // Step 2 goes back with Previous, and the unit starts as Count.
  await expect(page.getByTestId("form-back")).toHaveText("Previous");
  await expect(page.getByTestId("mf-unit")).toContainText("Count");
  await expect(page.getByTestId("call-panel")).toContainText('"unit": "Count"');
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
  await expectNoErrorAnswers(page, refusals);
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

test("logs__log-group-delete, log-group-deleted, log-groups-none: delete from the list, then the last of them", async ({ page, refusals }) => {
  // A group with deletion protection is refused its delete, and the dialog says so.
  refusals.allow(/400 DELETE .*\/logs\/groups\/%2Flarder%2Fkeep$/);
  await go(page, "/resources/logs/groups", "Log groups");
  const row = (name: string) => page.locator('[data-slot="data-table"] tbody tr[data-row]').filter({ hasText: name });
  await row("/larder/scratch").getByRole("checkbox").click();
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("DeleteLogGroup deletes /larder/scratch and permanently deletes all the archived log events associated with it.");
  await shot(page, "logs__log-group-delete");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("DeleteLogGroup returned 200. /larder/scratch and the log events in it are gone.");
  await expectNoErrorAnswers(page, refusals);
  await expect(row("/larder/scratch")).toHaveCount(0);
  await shot(page, "logs__log-group-deleted");
  // A protected group is the service's refusal, said in the dialog.
  cli(["logs", "create-log-group", "--log-group-name", "/larder/keep", "--deletion-protection-enabled"]);
  await page.reload();
  await row("/larder/keep").getByRole("checkbox").click();
  await page.getByTestId("action-delete").click();
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("delete-dialog-error")).toContainText("DeleteLogGroup was refused");
  await expectNoErrorAnswers(page, refusals);
  await page.keyboard.press("Escape");
  const all = json<{ logGroups: { logGroupName: string }[] }>(["logs", "describe-log-groups"]).logGroups.map((g) => g.logGroupName);
  cli(["logs", "put-log-group-deletion-protection", "--log-group-identifier", "/larder/keep", "--no-deletion-protection-enabled"]);
  for (const g of all) cli(["logs", "delete-log-group", "--log-group-name", g]);
  await go(page, "/resources/logs/groups", "Log groups");
  await expect(page.getByTestId("list-empty")).toContainText("No log groups");
  await shot(page, "logs__log-groups-none");
});

test("kinesis__streams-list: the data streams with their sentence and the consumer by name", async ({ page }) => {
  await go(page, "/resources/kinesis/streams", "Data streams");
  await expect(page.getByTestId("list-sentence")).toContainText("3 data streams: 2 provisioned and 1 on-demand. clickstream has 1 registered consumer, search-indexer.");
  const row = (name: string) => page.locator('[data-slot="data-table"] tbody tr[data-row]').filter({ hasText: name });
  await expect(row("clickstream")).toContainText("Active");
  await expect(row("clickstream")).toContainText("Provisioned");
  await expect(row("orders-feed")).toContainText("On-demand");
  // Oldest first, not by name: clickstream was made first, and inventory-feed sorts before orders-feed only by name.
  expect(await page.locator('[data-slot="data-table"] tbody tr[data-row]').first().textContent()).toContain("clickstream");
  await expect(page.getByTestId("action-delete")).toBeDisabled();
  await row("clickstream").getByRole("checkbox").click();
  await shot(page, "kinesis__streams-list");
});

test("kinesis__stream-create, stream-create-refused, stream-create-provisioned: the capacity cards, the call, the name in use, the estimator", async ({ page, refusals }) => {
  // The name in use is Kinesis's refusal, shown on the form.
  refusals.allow(/400 POST .*\/kinesis\/streams\/op\/CreateStream$/);
  await go(page, "/resources/kinesis/streams?create=yes", "Create data stream");
  await page.getByTestId("stream-name").fill("orders-feed");
  await page.getByTestId("section-tags").getByRole("textbox").first().fill("stack");
  await page.getByTestId("section-tags").getByRole("textbox").nth(1).fill("larder");
  await expect(page.getByTestId("stream-create")).toContainText("An on-demand stream starts with 4 shards in doze-aws.");
  await expect(page.getByTestId("stream-create")).toContainText("clickstream and inventory-feed already exist in us-east-1.");
  await expect(page.getByTestId("call-panel")).toContainText('"StreamMode": "ON_DEMAND"');
  await expect(page.getByTestId("call-panel")).toContainText('"stack": "larder"');
  await shot(page, "kinesis__stream-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("create-refused")).toContainText("CreateStream answered ResourceInUseException");
  await expect(page.getByTestId("section-name")).toContainText("ResourceInUseException");
  await shot(page, "kinesis__stream-create-refused");
  // Provisioned: the estimator sizes the shards from the traffic. (The page is still folding its sections and scrolling to the
  // refused name; a click that lands as it moves is not a click: WebKit under load lost this one.)
  await settled(page);
  await page.getByTestId("mode-PROVISIONED").click();
  await page.getByTestId("stream-name").fill("audit-feed");
  await expect(page.getByTestId("est-rate")).toHaveValue("3,000");
  await expect(page.getByTestId("est-shards")).toHaveText("6");
  await expect(page.getByTestId("section-capacity")).toContainText("clickstream runs on 2 shards.");
  await expect(page.getByTestId("est-result")).toContainText("max(6,000 / 1,024, 12,000 / 2,048) = 5.86, rounded up");
  await expect(page.getByTestId("stream-shards")).toHaveValue("6");
  await expect(page.getByTestId("call-panel")).toContainText('"ShardCount": 6');
  await shot(page, "kinesis__stream-create-provisioned");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("CreateStream returned 200");
  const made = json<{ StreamDescriptionSummary: { OpenShardCount: number; StreamModeDetails: { StreamMode: string } } }>(["kinesis", "describe-stream-summary", "--stream-name", "audit-feed"]).StreamDescriptionSummary;
  expect([made.OpenShardCount, made.StreamModeDetails.StreamMode]).toEqual([6, "PROVISIONED"]);
});

test("kinesis__stream-detail and the edits: capacity, retention, encryption, shard-level metrics", async ({ page }) => {
  await go(page, kstream("clickstream"), "clickstream");
  await expect(page.getByTestId("detail-badge")).toHaveText("Active");
  await expect(page.getByTestId("detail-sentence")).toContainText("clickstream has 2 open shards, keeps records for 24 hours, and has 1 registered consumer, search-indexer, which reads it with GetRecords.");
  await expect(page.getByTestId("wired-strip")).toContainText("search-indexer");
  await expect(page.getByTestId("details")).toContainText("arn:aws:kinesis:us-east-1:");
  await expect(page.getByTestId("section-capacity")).toContainText("2 shards is 2 MiB per second and 2,000 records per second in, 4 MiB per second out");
  await expect(page.getByTestId("section-retention")).toContainText("The oldest record the Data Viewer can reach on shardId-");
  await expect(page.getByTestId("section-encryption")).toContainText("Stored, not applied locally.");
  await expect(page.getByTestId("section-metrics")).toContainText("None enabled");
  await expect(page.getByTestId("section-tags")).toContainText("larder");
  await expect(page.getByTestId("recent-calls")).toContainText("PutRecord");
  await expect(page.getByTestId("recent-calls").getByTestId("answer-words").filter({ hasText: "· 1 record" }).first()).toBeVisible();
  await shot(page, "kinesis__stream-detail");
  // Capacity mode: a switch changes the mode and keeps the shards.
  await page.getByTestId("edit-capacity").click();
  await expect(page.getByTestId("capacity-dialog")).toContainText("Switching keeps the 2 shards the stream has");
  await shot(page, "kinesis__stream-capacity-edit");
  await page.getByTestId("cap-ON_DEMAND").click();
  await page.getByTestId("capacity-dialog-save").click();
  await expect(page.getByTestId("section-capacity").getByTestId("field-Capacity mode")).toHaveText("On-demand");
  cli(["kinesis", "update-stream-mode", "--stream-arn", json<{ StreamDescriptionSummary: { StreamARN: string } }>(["kinesis", "describe-stream-summary", "--stream-name", "clickstream"]).StreamDescriptionSummary.StreamARN, "--stream-mode-details", "StreamMode=PROVISIONED"]);
  await page.reload();
  // Retention: hours, with the day equivalent; Increase is chosen by the direction.
  await page.getByTestId("edit-retention").click();
  await page.getByTestId("retention-hours").fill("72");
  await expect(page.getByTestId("retention-dialog")).toContainText("That is 3 days. The API takes hours (RetentionPeriodHours).");
  await expect(page.getByTestId("retention-dialog-call")).toContainText("IncreaseStreamRetentionPeriod");
  await shot(page, "kinesis__stream-retention-edit");
  await page.getByTestId("retention-dialog-save").click();
  await expect(page.getByTestId("section-retention")).toContainText("72 hours");
  // Encryption: on, with a key.
  await page.getByTestId("edit-encryption").click();
  await page.getByTestId("sse-switch").click();
  await page.getByTestId("sse-key").click();
  await page.getByRole("option", { name: "alias/larder-app" }).click();
  await expect(page.getByTestId("encryption-dialog-call")).toContainText("StartStreamEncryption");
  // doze-aws does not look the key up, so the dialog must not say that a missing key is refused.
  await expect(page.getByTestId("encryption-dialog")).not.toContainText("must exist and be enabled");
  await expect(page.getByTestId("encryption-dialog")).toContainText("does not look the key up");
  await shot(page, "kinesis__stream-encryption-edit");
  await page.getByTestId("encryption-dialog-save").click();
  await expect(page.getByTestId("section-encryption")).toContainText("alias/larder-app");
  // Shard-level metrics.
  await page.getByTestId("edit-metrics").click();
  await expect(page.getByTestId("metrics-dialog")).toContainText("ReadProvisionedThroughputExceeded");
  await shot(page, "kinesis__stream-metrics-edit");
  await page.getByTestId("metric-IncomingBytes").click();
  await page.getByTestId("metrics-dialog-save").click();
  await expect(page.getByTestId("section-metrics")).toContainText("IncomingBytes");
  expect(json<{ StreamDescriptionSummary: { RetentionPeriodHours: number; EncryptionType: string; EnhancedMonitoring: { ShardLevelMetrics: string[] }[] } }>(["kinesis", "describe-stream-summary", "--stream-name", "clickstream"]).StreamDescriptionSummary).toMatchObject({ RetentionPeriodHours: 72, EncryptionType: "KMS", EnhancedMonitoring: [{ ShardLevelMetrics: ["IncomingBytes"] }] });
});

test("kinesis__stream-data-viewer, data-viewer-none, record-detail: a shard from its trim horizon, a position with nothing, one record opened", async ({ page }) => {
  await go(page, kstream("clickstream", "?tab=viewer"), "clickstream");
  await expect(page.getByTestId("stream-viewer")).toContainText("Shows the records in one shard without a consumer application.");
  // Every shard is read until the one that holds records (the six were spread by partition key).
  const shards = json<{ Shards: { ShardId: string }[] }>(["kinesis", "list-shards", "--stream-name", "clickstream"]).Shards.map((s) => s.ShardId);
  let found = shards[0] ?? "";
  for (const id of shards) {
    const it = json<{ ShardIterator: string }>(["kinesis", "get-shard-iterator", "--stream-name", "clickstream", "--shard-id", id, "--shard-iterator-type", "TRIM_HORIZON"]).ShardIterator;
    if (json<{ Records: unknown[] }>(["kinesis", "get-records", "--shard-iterator", it]).Records.length > 0) { found = id; break; }
  }
  await page.getByTestId("viewer-shard").click();
  await page.getByRole("option", { name: found }).click();
  await page.getByTestId("viewer-get").click();
  await expect(page.getByTestId("viewer-records")).toContainText("Approximate arrival timestamp");
  await expect(page.getByTestId("viewer-count")).toContainText(`from ${found}`);
  await expect(page.getByTestId("viewer-showing")).toContainText("Showing");
  await shot(page, "kinesis__stream-data-viewer");
  await page.getByTestId("record-open").first().click();
  await expect(page.getByTestId("record-drawer")).toContainText("Encryption type");
  await expect(page.getByTestId("record-data")).toContainText("event");
  await page.getByTestId("record-view-json").click();
  await shot(page, "kinesis__record-detail");
  await page.getByTestId("record-done").click();
  // Latest starts after the newest record: nothing.
  await page.getByTestId("viewer-position").click();
  await page.getByRole("option", { name: "Latest" }).click();
  await page.getByTestId("viewer-get").click();
  await expect(page.getByTestId("viewer-empty")).toContainText("GetRecords returned nothing from this position.");
  await shot(page, "kinesis__stream-data-viewer-none");
});

test("kinesis__stream-sharing, sharing-none, sharing-create: no policy, the visual editor, the policy kept", async ({ page }) => {
  await go(page, kstream("orders-feed", "?tab=sharing"), "orders-feed");
  await expect(page.getByTestId("sharing-empty")).toContainText("No resource policy is attached, so no other account or principal has been given access to this stream.");
  // The Summary stays above the tabs, the policy is a field, and the note says what doze-aws does with a policy (stores it, evaluates nothing).
  await expect(page.getByTestId("details")).toContainText("Stream ARN");
  await expect(page.getByTestId("field-Resource-based policy")).toHaveText("None attached to orders-feed.");
  await expect(page.getByTestId("sharing-empty")).toContainText("does not evaluate a stream's resource policy");
  await shot(page, "kinesis__stream-sharing-none");
  await page.getByTestId("sharing-create").click();
  await expect(page.locator("h1").first()).toHaveText("Visual policy editor");
  await page.getByTestId("share-principal").fill(`arn:aws:iam::${server.account}:role/report-job`);
  await expect(page.getByTestId("sharing-create-page")).toContainText("The role report-job already exists in this account.");
  await expect(page.getByTestId("call-panel")).toContainText("PutResourcePolicy");
  await expect(page.getByTestId("share-policy")).toContainText("kinesis:GetShardIterator");
  await shot(page, "kinesis__stream-sharing-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("sharing-banner")).toContainText("PutResourcePolicy returned 200");
  await expect(page.getByTestId("sharing-policy")).toContainText("may call kinesis:DescribeStreamSummary");
  await expect(page.getByTestId("sharing-policy")).toContainText("does not evaluate a stream's resource policy");
  await shot(page, "kinesis__stream-sharing");
  expect(cli(["kinesis", "get-resource-policy", "--resource-arn", json<{ StreamDescriptionSummary: { StreamARN: string } }>(["kinesis", "describe-stream-summary", "--stream-name", "orders-feed"]).StreamDescriptionSummary.StreamARN])).toContain("report-job");
});

test("kinesis__stream-tags-none, stream-tags-edit, stream-shards-edit: a stream with no tags, tags changed on a page, shards scaled", async ({ page }) => {
  await go(page, kstream("inventory-feed"), "inventory-feed");
  await expect(page.getByTestId("tags-empty")).toContainText("inventory-feed has no tags. Tags are key-value labels for organising streams and tracking their cost.");
  await shot(page, "kinesis__stream-tags-none");
  await go(page, kstream("clickstream", "?view=tags"), "Manage tags");
  await expect(page.getByTestId("manage-tags-page")).toContainText("3 tags are saved on this stream.");
  await page.getByRole("button", { name: "Add new tag" }).click();
  const inputs = page.getByTestId("tags-kv").getByRole("textbox");
  await inputs.nth(6).fill("team");
  await inputs.nth(7).fill("storefront");
  await expect(page.getByTestId("call-panel")).toContainText("AddTagsToStream");
  await expect(page.getByTestId("manage-tags-page")).toContainText("The team row is new.");
  await shot(page, "kinesis__stream-tags-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("tags-banner")).toContainText("Tags saved.");
  expect(json<{ Tags: { Key: string }[] }>(["kinesis", "list-tags-for-stream", "--stream-name", "clickstream"]).Tags.map((t) => t.Key)).toContain("team");
  // Shards: UNIFORM_SCALING, before and after.
  await page.getByTestId("edit-shards").click();
  await page.getByTestId("shard-target").fill("4");
  await expect(page.getByTestId("shards-dialog")).toContainText("UNIFORM_SCALING");
  await expect(page.getByTestId("shards-after")).toContainText("2 now, 4 after");
  await shot(page, "kinesis__stream-shards-edit");
  await page.getByTestId("shards-dialog-save").click();
  await expect(page.getByTestId("field-Provisioned shards")).toHaveText("4");
});

test("kinesis__stream-delete, stream-deleted, streams-none: delete from the list with the consumer named, then the last of them", async ({ page, refusals }) => {
  await go(page, "/resources/kinesis/streams", "Data streams");
  const row = (name: string) => page.locator('[data-slot="data-table"] tbody tr[data-row]').filter({ hasText: name });
  await row("clickstream").getByRole("checkbox").click();
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("This deletes clickstream and all its shards and data. Its 4 tags are dissociated from it. Applications that still use it will get ResourceNotFoundException.");
  await expect(page.getByTestId("delete-consumers")).toContainText("clickstream has 1 registered consumer, search-indexer. Deleting the stream drops its consumer registrations too.");
  await shot(page, "kinesis__stream-delete");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Deleted clickstream. Its shards, its records and its tags are gone, and the consumer search-indexer was deregistered with it.");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("list-sentence")).toContainText("3 data streams remain. Anything still writing to clickstream now gets ResourceNotFoundException.");
  await shot(page, "kinesis__stream-deleted");
  for (const n of ["orders-feed", "inventory-feed", "audit-feed"]) cli(["kinesis", "delete-stream", "--stream-name", n]);
  await go(page, "/resources/kinesis/streams", "Data streams");
  await expect(page.getByTestId("list-empty")).toContainText("Create a data stream to start ingesting records.");
  await shot(page, "kinesis__streams-none");
});
