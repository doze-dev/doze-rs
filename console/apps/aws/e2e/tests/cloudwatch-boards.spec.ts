// The 26 CloudWatch boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder"
// world with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board.
// No test may log a console error: a not-found page is the one refusal, and says so.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { CUT, expect, test } from "./fixtures";
import { seedLarderCloudwatch } from "./cloudwatch-seed";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-cloudwatch");
const BASE = Number(process.env.E2E_BASE_PORT ?? 46100);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });

let server: Running;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

/** Photographs the page as the board is: light, then dark, in Chromium only. */
async function shot(page: Page, board: string) {
  if (browserKind !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
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
const alarm = (name: string, search = "") => `/resources/cloudwatch/alarms/${name}${search}`;
const dash = (name: string, search = "") => `/resources/cloudwatch/dashboards/${name}${search}`;
const choose = async (page: Page, combobox: string, option: string | RegExp) => {
  await page.getByRole("combobox", { name: combobox, exact: true }).click();
  await page.getByRole("option", { name: option }).first().click();
};
const pick = async (page: Page, trigger: string, option: string | RegExp) => {
  await page.getByTestId(trigger).click();
  await page.getByRole("option", { name: option }).click();
};
const code = (page: Page) => page.getByTestId("call-panel-code");

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  seedLarderCloudwatch(server.endpoint, server.account);
});
test.afterAll(async () => {
  await server.stop();
});

test("cloudwatch__overview: alarms by AWS service, the recent alarms, and where to go", async ({ page }) => {
  await go(page, "/resources/cloudwatch", "CloudWatch");
  await expect(page.getByTestId("overview-lead")).toContainText(`account ${server.account} in us-east-1`);
  await expect(page.getByTestId("overview-lead")).toContainText("Larder/Orders and Larder/Queues");
  const service = page.getByRole("table", { name: "Alarms by AWS service" });
  await expect(service.locator("tr[data-row]", { hasText: "AWS Lambda" })).toContainText("1");
  await expect(service.locator("tr[data-row]", { hasText: "Amazon API Gateway" })).toContainText("1");
  const recent = page.getByRole("table", { name: "Recent alarms" });
  await expect(recent.locator("tr[data-row]").first()).toContainText("dlq-not-empty");
  await expect(recent.locator("tr[data-row]").first()).toContainText("ALARM");
  await expect(page.getByTestId("card-alarms")).toContainText("5 alarms, 1 in ALARM");
  await expect(page.getByTestId("card-metrics")).toContainText("All metrics: 3 namespaces");
  await expect(page.getByTestId("card-dashboards")).toContainText("2 custom dashboards");
  await shot(page, "cloudwatch__overview");
  await page.getByTestId("card-alarms").click();
  await expect(page.locator("h1").first()).toHaveText("Alarms");
});

test("cloudwatch__alarms-list: the state chips with counts, the condition, a row chosen for Actions > Delete", async ({ page }) => {
  await go(page, "/resources/cloudwatch/alarms", "Alarms");
  const table = page.getByRole("table", { name: "Alarms" });
  await expect(table.locator("tr[data-row]")).toHaveCount(5);
  await expect(page.getByTestId("chip-all")).toContainText("5");
  await expect(page.getByTestId("chip-ALARM")).toContainText("1");
  await expect(page.getByTestId("chip-OK")).toContainText("3");
  await expect(page.getByTestId("chip-INSUFFICIENT_DATA")).toContainText("1");
  const dlq = table.locator("tr[data-row]", { hasText: "dlq-not-empty" });
  await expect(dlq).toContainText("Metric alarm");
  await expect(dlq).toContainText("DlqMessages >= 1 for 1 datapoints within 1 minute");
  await expect(table.locator("tr[data-row]", { hasText: "resize-errors" })).toContainText("Errors >= 3 for 2 datapoints within 15 minutes");
  await expect(table.locator("tr[data-row]", { hasText: "orders-placed-low" })).toContainText("OrdersPlaced < 5 for 3 datapoints within 15 minutes");
  await expect(dlq).toContainText("Yes");
  await page.getByTestId("chip-OK").click();
  await expect(table.locator("tr[data-row]")).toHaveCount(3);
  await page.getByTestId("chip-all").click();
  await page.getByTestId("list-search").fill("api-5xx");
  await expect(table.locator("tr[data-row]")).toHaveCount(1);
  await page.getByTestId("list-search").fill("");
  await dlq.getByRole("checkbox").check();
  await expect(page.getByTestId("selected-count")).toContainText("selected");
  await shot(page, "cloudwatch__alarms-list");
});

test("cloudwatch__alarm-delete: the dialog says what stops, then deleting leaves the list without it", async ({ page }) => {
  cli(["cloudwatch", "put-metric-alarm", "--alarm-name", "scratch-alarm", "--namespace", "Larder/Queues", "--metric-name", "DlqMessages", "--dimensions", "Name=QueueName,Value=pick-lists-dlq", "--statistic", "Maximum", "--period", "60", "--evaluation-periods", "1", "--threshold", "1", "--comparison-operator", "GreaterThanOrEqualToThreshold", "--alarm-actions", `arn:aws:sns:us-east-1:${server.account}:alerts`]);
  await go(page, "/resources/cloudwatch/alarms", "Alarms");
  const table = page.getByRole("table", { name: "Alarms" });
  await table.locator("tr[data-row]", { hasText: "dlq-not-empty" }).getByRole("checkbox").check();
  await page.getByTestId("action-delete").click();
  const dialog = page.getByTestId("delete-dialog");
  await expect(dialog).toContainText("Delete dlq-not-empty?");
  await expect(dialog).toContainText("Delete dlq-not-empty. It stops watching pick-lists-dlq and its actions will not run again.");
  await expect(dialog.getByRole("button", { name: "Delete", exact: true })).toBeVisible();
  await shot(page, "cloudwatch__alarm-delete");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await table.locator("tr[data-row]", { hasText: "dlq-not-empty" }).getByRole("checkbox").uncheck();
  await table.locator("tr[data-row]", { hasText: "scratch-alarm" }).getByRole("checkbox").check();
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("Delete scratch-alarm.");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Alarm scratch-alarm deleted.");
  await expect(table.locator("tr[data-row]", { hasText: "scratch-alarm" })).toHaveCount(0);
  expect(cli(["cloudwatch", "describe-alarms", "--alarm-names", "scratch-alarm"])).not.toContain("scratch-alarm");
});

test("cloudwatch__alarm-detail: the state, the graph with its threshold, the details and the actions per state", async ({ page }) => {
  await go(page, alarm("dlq-not-empty"), "dlq-not-empty");
  await expect(page.getByTestId("alarm-page")).toContainText("ALARM");
  await expect(page.getByTestId("alarm-page")).toContainText("Metric alarm");
  await expect(page.locator('[data-slot="sentence"], p').filter({ hasText: "The dead-letter queue pick-lists-dlq holds a message. ALARM since" }).first()).toBeVisible();
  await expect(page.getByTestId("alarm-graph")).toBeVisible();
  await expect(page.getByTestId("alarm-chart")).toContainText("DlqMessages >= 1");
  for (const r of ["1h", "3h", "12h", "1d", "3d", "1w", "custom"]) await expect(page.getByTestId(`range-${r}`)).toBeVisible();
  await page.getByTestId("range-12h").click();
  await expect(page.getByTestId("range-12h")).toHaveAttribute("data-state", "on");
  await expect(page.getByTestId("alarm-type")).toHaveText("Metric alarm");
  await expect(page.getByTestId("alarm-reason")).toContainText("Threshold Crossed");
  await expect(page.getByTestId("alarm-threshold")).toHaveText("DlqMessages >= 1 for 1 datapoints within 1 minute");
  await expect(page.getByTestId("alarm-metric-row")).toContainText("Larder/Queues");
  await expect(page.getByTestId("alarm-metric-row")).toContainText("QueueName = pick-lists-dlq");
  await expect(page.getByTestId("alarm-statistic")).toHaveText("Maximum, 1 minute");
  await expect(page.getByTestId("alarm-datapoints")).toHaveText("1 out of 1");
  await expect(page.getByTestId("alarm-missing")).toHaveText("ignore");
  await expect(page.getByTestId("alarm-arn")).toContainText(`arn:aws:cloudwatch:us-east-1:${server.account}:alarm:dlq-not-empty`);
  await expect(page.getByTestId("actions-enabled")).toContainText("Yes");
  await expect(page.getByTestId("actions-alarm")).toContainText("Send a notification to alerts");
  await expect(page.getByTestId("actions-alarm")).toContainText(`arn:aws:sns:us-east-1:${server.account}:alerts`);
  await expect(page.getByTestId("actions-ok")).toHaveText("No actions");
  await expect(page.getByTestId("actions-insufficient_data")).toHaveText("No actions");
  await expect(page.getByTestId("actions-explainer")).toContainText("An alarm runs actions only when its state changes.");
  await expect(page.getByTestId("actions-explainer")).toContainText("It moved from");
  await page.getByTestId("range-3h").click();
  await shot(page, "cloudwatch__alarm-detail");
});

test("cloudwatch__alarm-history: the entries, the type filter and the window", async ({ page }) => {
  await go(page, alarm("dlq-not-empty", "?tab=history"), "dlq-not-empty");
  const table = page.getByRole("table", { name: "Alarm history" });
  await expect(table.locator("tr[data-row]").first()).toContainText("Action");
  await expect(table.locator("tr[data-row]", { hasText: "ConfigurationUpdate" })).toContainText('Alarm "dlq-not-empty" created');
  await expect(table.locator("tr[data-row]", { hasText: "StateUpdate" }).first()).toContainText("Alarm updated from");
  await expect(table).toContainText(`Successfully executed action arn:aws:sns:us-east-1:${server.account}:alerts`);
  await shot(page, "cloudwatch__alarm-history");
  await page.getByTestId("chip-StateUpdate").click();
  await expect(table.locator("tr[data-row]", { hasText: "ConfigurationUpdate" })).toHaveCount(0);
  await expect(table.locator("tr[data-row]").first()).toContainText("StateUpdate");
  await page.getByTestId("chip-Action").click();
  await expect(table.locator("tr[data-row]")).toHaveCount(1);
  await expect(page.getByTestId("history-explainer")).toContainText("Three kinds of entry");
  await expect(page.getByRole("button", { name: "StartDate, UTC" })).toBeVisible();
  await expect(page.getByRole("button", { name: "EndDate, UTC" })).toBeVisible();
});

test("cloudwatch__alarm-tags and alarm-tags-none: tags to manage, and the state of an alarm with none", async ({ page }) => {
  await go(page, alarm("orders-error-rate", "?tab=tags"), "orders-error-rate");
  await expect(page.getByTestId("alarm-page")).toContainText("Created");
  await expect(page.getByRole("heading", { name: "No tags" })).toBeVisible();
  await expect(page.getByText("orders-error-rate has no tags. Tags are key-value labels for organising alarms.")).toBeVisible();
  await shot(page, "cloudwatch__alarm-tags-none");
  await go(page, alarm("dlq-not-empty", "?tab=tags"), "dlq-not-empty");
  const view = page.getByTestId("tags-view");
  await expect(view).toContainText("3 tags");
  await expect(view).toContainText("larder");
  await page.getByTestId("tags-edit-open").click();
  await expect(page.getByTestId("tags-edit")).toContainText("Manage tags");
  await expect(page.getByTestId("tags-edit")).toContainText("Tags are key-value pairs that help you organize and categorize your alarms.");
  await expect(page.getByTestId("tags-rules")).toContainText("Up to 50 tags per alarm; keys are 1 to 128 characters and values 0 to 256.");
  await shot(page, "cloudwatch__alarm-tags");
  await page.getByTestId("tags-cancel").click();
  // Saving a new tag calls TagResource; the alarm has it afterwards.
  await page.getByTestId("tags-edit-open").click();
  await page.getByTestId("tags-kv-add").click();
  const boxes = page.getByTestId("tags-edit").getByRole("textbox");
  const n = await boxes.count();
  await boxes.nth(n - 2).fill("owner");
  await boxes.nth(n - 1).fill("kitchen");
  await page.getByTestId("tags-save").click();
  await expect(page.getByTestId("tags-view")).toContainText("owner");
  expect(cli(["cloudwatch", "list-tags-for-resource", "--resource-arn", `arn:aws:cloudwatch:us-east-1:${server.account}:alarm:dlq-not-empty`])).toContain("kitchen");
});

test("cloudwatch__alarm-step1 to step4: the wizard, the call each step makes, and the alarm it creates", async ({ page }) => {
  await go(page, "/resources/cloudwatch/alarms", "Alarms");
  await page.getByTestId("create-open").click();
  await expect(page.locator("h1").first()).toHaveText("Create alarm");
  await expect(page.getByText("Step 1 of 4. Choose the metric to watch and the condition that puts the alarm in ALARM.")).toBeVisible();
  for (const s of ["Specify metric and conditions", "Configure actions", "Add name and description", "Preview and create"]) await expect(page.getByTestId("alarm-wizard").getByText(s).first()).toBeVisible();
  await page.getByTestId("form-submit").click();
  await expect(page.getByText("Choose the metric to watch.").first()).toBeVisible();
  await choose(page, "Namespace", "Larder/Queues");
  await choose(page, "Metric name", "DlqMessages");
  await choose(page, "Dimensions", "QueueName=pick-lists-dlq");
  await pick(page, "stat", "Maximum");
  await pick(page, "period", "1 minute");
  await page.getByTestId("metric-done").click();
  await expect(page.getByTestId("metric-rows")).toContainText("QueueName");
  await expect(page.getByTestId("wizard-graph")).toBeVisible();
  await expect(page.getByTestId("kind-anomaly")).toBeVisible();
  await expect(page.getByTestId("op-GreaterThanOrEqualToThreshold")).toBeChecked();
  await expect(code(page)).toContainText('"Namespace": "Larder/Queues"');
  await expect(code(page)).toContainText('"Statistic": "Maximum"');
  await expect(code(page)).toContainText('"Period": 60');
  await expect(code(page)).toContainText('"ComparisonOperator": "GreaterThanOrEqualToThreshold"');
  await expect(code(page)).not.toContainText("AlarmName");
  await expect(page.getByText("Period is 10, 20, 30, or a multiple of 60 seconds.")).toBeVisible();
  await shot(page, "cloudwatch__alarm-step1");
  // Metric math: an expression over the metric, which the alarm watches.
  await page.getByTestId("math-toggle").click();
  await page.getByTestId("math-expression").fill("m1 * 2");
  await expect(code(page)).toContainText('"Expression": "m1 * 2"');
  await page.getByTestId("math-toggle").click();
  await expect(code(page)).toContainText('"MetricName": "DlqMessages"');
  await page.getByTestId("form-submit").click();
  await expect(page.getByText("Step 2 of 4. Choose what happens when the alarm changes state.")).toBeVisible();
  await expect(page.getByTestId("section-notification")).toContainText("Send a message to an SNS topic when the alarm enters a state.");
  await page.getByTestId("form-submit").click();
  await expect(page.getByText("Choose the topic to notify.").first()).toBeVisible();
  await page.getByRole("combobox", { name: "Topic" }).click();
  await page.getByRole("option", { name: "alerts" }).click();
  await expect(page.getByTestId("topic-arn")).toHaveText(`arn:aws:sns:us-east-1:${server.account}:alerts`);
  await expect(page.getByTestId("topic-note")).toContainText("CloudWatch does not test the topic.");
  await expect(page.getByTestId("topic-note")).toContainText("alerts delivers to the Lambda function send-email");
  await expect(code(page)).toContainText(`arn:aws:sns:us-east-1:${server.account}:alerts`);
  await expect(code(page)).toContainText('"ActionsEnabled": true');
  await expect(page.getByRole("button", { name: "Add Lambda action" })).toBeEnabled();
  for (const b of ["Auto Scaling action", "EC2 action", "Add Systems Manager action", "Add investigation action"]) await expect(page.getByRole("button", { name: b })).toBeDisabled();
  await expect(page.getByTestId("actions-off-reason")).toContainText("SNS topics and Lambda functions only");
  await shot(page, "cloudwatch__alarm-step2");
  await page.getByTestId("form-submit").click();
  await expect(page.getByText("Step 3 of 4. Name the alarm and say what it means.")).toBeVisible();
  await page.getByTestId("form-submit").click();
  await expect(page.getByText("Enter the alarm name.")).toBeVisible();
  await page.getByTestId("alarm-name").fill("dlq-not-empty");
  await page.getByTestId("form-submit").click();
  await expect(page.getByText("An alarm named dlq-not-empty already exists in this Region.")).toBeVisible();
  await page.getByTestId("alarm-name").fill("pick-lists-dlq-watch");
  await page.getByTestId("alarm-description").fill("The dead-letter queue pick-lists-dlq holds a message. Look at the message first.");
  await expect(code(page)).toContainText('"AlarmName": "pick-lists-dlq-watch"');
  await shot(page, "cloudwatch__alarm-step3");
  await page.getByTestId("form-submit").click();
  await expect(page.getByText("Step 4 of 4.")).toBeVisible();
  await expect(page.getByTestId("summary-metric")).toContainText("Larder/Queues · DlqMessages");
  await expect(page.getByTestId("summary-metric")).toContainText("DlqMessages >= 1 for 1 datapoints within 1 minute");
  await expect(page.getByTestId("summary-actions")).toContainText("Send a notification to alerts");
  await expect(page.getByTestId("summary-name")).toContainText("pick-lists-dlq-watch");
  await expect(page.getByText("Creating it sets its state to INSUFFICIENT_DATA until the first evaluation.")).toBeVisible();
  await expect(code(page)).toContainText('"AlarmName": "pick-lists-dlq-watch"');
  await expect(code(page)).toContainText('"AlarmActions"');
  await shot(page, "cloudwatch__alarm-step4");
  await page.getByTestId("edit-summary-name").click();
  await expect(page.getByTestId("alarm-name")).toHaveValue("pick-lists-dlq-watch");
  await page.getByTestId("form-submit").click();
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("pick-lists-dlq-watch");
  await expect(page.getByTestId("created-banner")).toContainText("Alarm pick-lists-dlq-watch created.");
  expect(cli(["cloudwatch", "describe-alarms", "--alarm-names", "pick-lists-dlq-watch"])).toContain("Look at the message first.");
  // Edit: the same pages over the alarm, its name kept.
  await page.getByTestId("actions-menu").click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await expect(page.locator("h1").first()).toHaveText("Edit alarm");
  await page.getByTestId("form-submit").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("alarm-name")).toBeDisabled();
  await page.getByTestId("form-submit").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("updated-banner")).toContainText("updated");
});

test("cloudwatch__metrics-all and metrics-none: namespaces by dimension set, the search, and the notes", async ({ page }) => {
  await go(page, "/resources/cloudwatch/metrics", "All metrics");
  const custom = page.getByRole("table", { name: "Custom namespaces" });
  await expect(custom.locator("tr[data-row]", { hasText: "Larder/Orders" })).toContainText("Environment");
  await expect(custom.locator("tr[data-row]", { hasText: "Larder/Orders" })).toContainText("2");
  await expect(custom.locator("tr[data-row]", { hasText: "Larder/Queues" })).toContainText("QueueName");
  await expect(page.getByRole("table", { name: "AWS namespaces" }).locator("tr[data-row]", { hasText: "AWS/Lambda" }).first()).toBeVisible();
  await expect(page.getByTestId("metrics-graph-empty")).toHaveText("No metrics selected.");
  await expect(page.getByTestId("retention-note")).toContainText("Samples are kept for 24 hours.");
  await expect(page.getByTestId("no-metrics-note")).toContainText("publish no metrics");
  for (const r of ["1h", "3h", "12h", "1d", "3d", "1w", "custom"]) await expect(page.getByTestId(`range-${r}`)).toBeVisible();
  await shot(page, "cloudwatch__metrics-all");
  await page.getByTestId("metric-search").fill("payments-latency");
  await expect(page.getByText("No metrics found").first()).toBeVisible();
  await expect(page.getByText("ListMetrics returned nothing for this search.").first()).toBeVisible();
  await shot(page, "cloudwatch__metrics-none");
  await page.getByTestId("metric-search").fill("pick-lists");
  await expect(page.getByTestId("custom-namespaces")).toContainText("Larder/Queues");
  await expect(page.getByTestId("custom-namespaces")).not.toContainText("Larder/Orders");
});

test("cloudwatch__metrics-graph and metrics-add-to-dashboard: tick two metrics, graph them, add the graph to a dashboard", async ({ page }) => {
  await go(page, "/resources/cloudwatch/metrics", "All metrics");
  await page.getByTestId("custom-namespaces").locator("tr[data-row]", { hasText: "Larder/Orders" }).getByTestId("dims-link").click();
  await expect(page.getByTestId("metrics-in")).toContainText("OrdersPlaced");
  const rows = page.getByRole("table", { name: "Metrics" }).locator("tr[data-row]");
  await rows.filter({ hasText: "OrdersPlaced" }).getByRole("checkbox").check();
  await rows.filter({ hasText: "OrdersFailed" }).getByRole("checkbox").check();
  await expect(page.getByTestId("tab-graphed")).toContainText("2");
  await expect(page.getByText("2 metrics from Larder/Orders, 5 minute periods, last 3 hours.")).toBeVisible();
  await page.getByTestId("tab-graphed").click();
  await expect(page.getByTestId("graphed-m1")).toContainText("Larder/Orders · Environment=prod");
  await expect(page.getByTestId("stat-m1")).toContainText("Sum");
  await expect(page.getByTestId("period-m1")).toContainText("5 minutes");
  await expect(page.getByTestId("metrics-graph")).toContainText("OrdersPlaced");
  await expect(page.getByTestId("metrics-graph")).toContainText("OrdersFailed");
  await expect(page.getByTestId("share")).toBeVisible();
  await expect(page.getByTestId("tab-options")).toBeVisible();
  await expect(page.getByTestId("tab-source")).toBeVisible();
  await expect(page.getByTestId("graphed-note")).toContainText("Statistic: Average, Minimum, Maximum, Sum, SampleCount");
  await shot(page, "cloudwatch__metrics-graph");
  // Math over the rows, drawn.
  await page.getByTestId("add-math").click();
  await page.getByTestId("expr-e1").fill("m2 / m1 * 100");
  await expect(page.getByTestId("metrics-graph")).toContainText("Expression");
  await page.getByTestId("remove-e1").click();
  await page.getByTestId("tab-source").click();
  await expect(page.getByTestId("source-tab")).toContainText('"OrdersPlaced"');
  await page.getByTestId("tab-graphed").click();
  await page.getByTestId("add-to-dashboard").click();
  const dialog = page.getByTestId("add-to-dashboard-dialog");
  await expect(dialog).toContainText("Add to dashboard");
  await expect(dialog).toContainText("Widget type");
  await expect(dialog.getByTestId("add-words")).toContainText("2 graphed metrics become one widget");
  await expect(dialog.getByText("Stored, not applied locally.")).toBeVisible();
  await shot(page, "cloudwatch__metrics-add-to-dashboard");
  await page.getByTestId("dashboard-select").click();
  await page.getByRole("option", { name: "larder-api" }).click();
  await page.getByTestId("widget-stacked").click();
  await page.getByTestId("add-to-dashboard-submit").click();
  await expect(page.locator("h1").first()).toHaveText("larder-api");
  await expect(page.getByTestId("done-banner")).toContainText("Widget added.");
  const body = JSON.parse((JSON.parse(cli(["cloudwatch", "get-dashboard", "--dashboard-name", "larder-api"])) as { DashboardBody: string }).DashboardBody) as { widgets: { properties: { stacked?: boolean; metrics: unknown[][] } }[] };
  expect(body.widgets).toHaveLength(2);
  expect(body.widgets[1]?.properties.stacked).toBe(true);
  expect(body.widgets[1]?.properties.metrics).toHaveLength(2);
  // The metrics page opened with a metric from a row's alarm icon starts the wizard on it.
  await go(page, "/resources/cloudwatch/metrics", "All metrics");
  await page.getByTestId("custom-namespaces").locator("tr[data-row]", { hasText: "Larder/Queues" }).getByTestId("dims-link").click();
  await page.getByRole("table", { name: "Metrics" }).locator("tr[data-row]", { hasText: "DlqMessages" }).getByRole("checkbox").check();
  await page.getByTestId("tab-graphed").click();
  await page.getByTestId("alarm-m1").click();
  await expect(page.locator("h1").first()).toHaveText("Create alarm");
  await expect(page.getByTestId("metric-rows")).toContainText("DlqMessages");
});

test("cloudwatch__dashboards-list, dashboard-create and dashboard-delete", async ({ page }) => {
  await go(page, "/resources/cloudwatch/dashboards", "Dashboards");
  const table = page.getByRole("table", { name: "Dashboards" });
  await expect(table.locator("tr[data-row]")).toHaveCount(2);
  await expect(table.locator("tr[data-row]", { hasText: "larder-ops" })).toContainText(/\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}/);
  await expect(page.getByTestId("dashboards-scope")).toContainText("A dashboard belongs to the Region it was made in.");
  await expect(page.getByTestId("dashboards-local")).toContainText("Stored, not applied locally.");
  await table.locator("tr[data-row]", { hasText: "larder-api" }).getByRole("checkbox").check();
  await expect(page.getByTestId("selected-count")).toContainText("selected");
  await shot(page, "cloudwatch__dashboards-list");
  await page.getByTestId("create-open").click();
  const dialog = page.getByTestId("create-dashboard-dialog");
  await expect(dialog).toContainText("Create new dashboard");
  await expect(dialog).toContainText("Up to 255 characters: A-Z, a-z, 0-9, hyphen and underscore.");
  await dialog.getByTestId("dashboard-name").fill("larder-fulfilment");
  await shot(page, "cloudwatch__dashboard-create");
  await dialog.getByTestId("dashboard-name").fill("");
  await dialog.getByTestId("create-dashboard-submit").click();
  await expect(dialog).toContainText("Enter a dashboard name.");
  await dialog.getByTestId("dashboard-name").fill("bad name");
  await dialog.getByTestId("create-dashboard-submit").click();
  await expect(dialog).toContainText("A name is up to 255 characters");
  await dialog.getByTestId("dashboard-name").fill("larder-ops");
  await dialog.getByTestId("create-dashboard-submit").click();
  await expect(dialog).toContainText("A dashboard named larder-ops already exists.");
  await dialog.getByTestId("dashboard-name").fill("scratch-board");
  await dialog.getByTestId("create-dashboard-submit").click();
  await expect(page.locator("h1").first()).toHaveText("scratch-board");
  await expect(page.getByTestId("done-banner")).toContainText("Dashboard scratch-board created.");
  await expect(page.getByTestId("dashboard-empty")).toBeVisible();
  expect(cli(["cloudwatch", "get-dashboard", "--dashboard-name", "scratch-board"])).toContain("widgets");
  await go(page, "/resources/cloudwatch/dashboards", "Dashboards");
  await table.locator("tr[data-row]", { hasText: "scratch-board" }).getByRole("checkbox").check();
  await page.getByTestId("action-delete").click();
  const del = page.getByTestId("delete-dialog");
  await expect(del).toContainText("Delete dashboards?");
  await expect(del).toContainText("Delete scratch-board. The dashboard and its widgets are removed.");
  await expect(del).toContainText("One call can delete up to 100 dashboards.");
  await shot(page, "cloudwatch__dashboard-delete");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Dashboard scratch-board deleted.");
  await expect(table.locator("tr[data-row]", { hasText: "scratch-board" })).toHaveCount(0);
});

test("cloudwatch__dashboard-view, dashboard-source and dashboard-add-widget: the widgets drawn, the source, the types", async ({ page }) => {
  await go(page, dash("larder-ops"), "larder-ops");
  await expect(page.getByText("Last saved")).toBeVisible();
  await expect(page.getByTestId("widget-0")).toContainText("Orders placed");
  await expect(page.getByTestId("widget-0").getByTestId("widget-chart")).toBeVisible();
  await expect(page.getByTestId("widget-1")).toContainText("resize-product-image errors");
  await expect(page.getByTestId("widget-2")).toContainText("DLQ depth");
  await expect(page.getByTestId("widget-value")).toHaveText("1");
  await expect(page.getByTestId("widget-3")).toContainText("Alarm status");
  await expect(page.getByTestId("widget-3")).toContainText("dlq-not-empty");
  await expect(page.getByTestId("widget-3")).toContainText("ALARM");
  await expect(page.getByTestId("widget-4")).toContainText("Dead-letter queue not empty");
  await expect(page.getByTestId("widget-count")).toContainText("This dashboard has 5 widgets;");
  await expect(page.getByTestId("dashboard-local")).toContainText("Stored, not applied locally.");
  await expect(page.getByTestId("save-dashboard")).toBeDisabled();
  await shot(page, "cloudwatch__dashboard-view");
  await page.getByTestId("source-open").click();
  const source = page.getByTestId("source-dialog");
  await expect(source).toContainText("View/edit source");
  await expect(source.getByTestId("source-editor")).toHaveValue(/"widgets"/);
  await expect(source.getByTestId("source-rules-words")).toContainText("a body holds 0 to 500 widgets on a 24-column grid");
  await shot(page, "cloudwatch__dashboard-source");
  await source.getByRole("button", { name: "Cancel" }).click();
  await page.getByTestId("add-widget").click();
  const choice = page.getByTestId("add-widget-dialog");
  for (const t of ["line", "stacked", "number", "gauge", "bar", "pie", "text", "alarm"]) await expect(choice.getByTestId(`widget-type-${t}`)).toBeVisible();
  await expect(choice).toContainText("Latest metric value with a sparkline");
  await shot(page, "cloudwatch__dashboard-add-widget");
});

test("cloudwatch__widget-line: pick a metric, create the widget, and Save dashboard writes it", async ({ page }) => {
  await go(page, dash("larder-api"), "larder-api");
  const before = JSON.parse((JSON.parse(cli(["cloudwatch", "get-dashboard", "--dashboard-name", "larder-api"])) as { DashboardBody: string }).DashboardBody) as { widgets: unknown[] };
  await page.getByTestId("add-widget").click();
  await page.getByTestId("widget-type-line").click();
  await page.getByTestId("add-widget-next").click();
  await expect(page.locator("h1").first()).toHaveText("Add metric graph");
  await expect(page.getByTestId("widget-create")).toBeDisabled();
  await page.getByTestId("custom-namespaces").locator("tr[data-row]", { hasText: "Larder/Orders" }).getByTestId("dims-link").click();
  await page.getByRole("table", { name: "Metrics" }).locator("tr[data-row]", { hasText: "OrdersPlaced" }).getByRole("checkbox").check();
  await page.getByTestId("tab-graphed").click();
  await expect(page.getByTestId("graphed-m1")).toContainText("Larder/Orders · Environment=prod");
  await expect(page.getByTestId("persist-words")).toContainText("Persist time range keeps this widget on the time range you pick");
  await expect(page.getByTestId("dashboard-local")).toContainText("Stored, not applied locally.");
  await page.getByTestId("persist-range").click();
  await shot(page, "cloudwatch__widget-line");
  await page.getByTestId("widget-create").click();
  await expect(page.locator("h1").first()).toHaveText("larder-api");
  await expect(page.getByTestId("done-banner")).toContainText("Widget added to the dashboard on this page.");
  await expect(page.getByText("You have changes that are not saved.")).toBeVisible();
  // Nothing is stored until Save dashboard.
  expect(JSON.parse((JSON.parse(cli(["cloudwatch", "get-dashboard", "--dashboard-name", "larder-api"])) as { DashboardBody: string }).DashboardBody).widgets).toHaveLength(before.widgets.length);
  await page.getByTestId("save-dashboard").click();
  await expect(page.getByTestId("done-banner")).toContainText("saved");
  const after = JSON.parse((JSON.parse(cli(["cloudwatch", "get-dashboard", "--dashboard-name", "larder-api"])) as { DashboardBody: string }).DashboardBody) as { widgets: { properties: { start?: string } }[] };
  expect(after.widgets).toHaveLength(before.widgets.length + 1);
  expect(after.widgets[after.widgets.length - 1]?.properties.start).toBe("-PT3H");
});

test("cloudwatch__widget-alarm-status and widget-text: the two widgets that need no metric", async ({ page }) => {
  await go(page, dash("larder-api"), "larder-api");
  await page.getByTestId("add-widget").click();
  await page.getByTestId("widget-type-alarm").click();
  await page.getByTestId("add-widget-next").click();
  const dialog = page.getByTestId("alarm-widget-dialog");
  await expect(dialog).toContainText("Tick the alarms to show in the widget.");
  await expect(dialog).toContainText("dlq-not-empty");
  await expect(dialog).toContainText("INSUFFICIENT_DATA");
  await expect(dialog).toContainText("up to 100 alarms");
  await dialog.getByTestId("alarm-dlq-not-empty").check();
  await dialog.getByTestId("alarm-resize-errors").check();
  await shot(page, "cloudwatch__widget-alarm-status");
  await dialog.getByTestId("widget-create").click();
  await expect(page.getByText("You have changes that are not saved.")).toBeVisible();
  await page.getByTestId("add-widget").click();
  await page.getByTestId("widget-type-text").click();
  await page.getByTestId("add-widget-next").click();
  const text = page.getByTestId("text-widget-dialog");
  await expect(text).toContainText("New text widget");
  await expect(text).toContainText("Format the text with Markdown.");
  await expect(text.getByTestId("widget-create")).toBeDisabled();
  await text.getByTestId("widget-markdown").fill("# On call\n\nRing the **kitchen**.");
  await text.getByTestId("widget-transparent").click();
  await shot(page, "cloudwatch__widget-text");
  await text.getByTestId("widget-create").click();
  await expect(page.getByTestId("dashboard-grid")).toContainText("Ring the kitchen.");
  await page.getByTestId("save-dashboard").click();
  await expect(page.getByTestId("done-banner")).toContainText("saved");
  const body = JSON.parse((JSON.parse(cli(["cloudwatch", "get-dashboard", "--dashboard-name", "larder-api"])) as { DashboardBody: string }).DashboardBody) as { widgets: { type: string; properties: { alarms?: string[]; background?: string } }[] };
  const kinds = body.widgets.map((w) => w.type);
  expect(kinds).toContain("alarm");
  expect(kinds).toContain("text");
  expect(body.widgets.find((w) => w.type === "alarm")?.properties.alarms).toHaveLength(2);
  expect(body.widgets.filter((w) => w.type === "text").pop()?.properties.background).toBe("transparent");
  // The source dialog replaces the draft; a rule broken is said, and a body that is not an object is refused.
  await page.getByTestId("source-open").click();
  await page.getByTestId("source-editor").fill('{"widgets":[{"type":"text","width":30,"properties":{"markdown":"x"}}]}');
  await expect(page.getByTestId("source-rules")).toContainText("width is 1 to 24");
  await page.getByTestId("source-editor").fill("[]");
  await expect(page.getByTestId("source-update")).toBeDisabled();
  await page.getByRole("button", { name: "Cancel" }).click();
});

// The service's own refusal (no such alarm, no such dashboard) is what a not-found page is: the browser logs it as a failed load.
test.describe("not found", () => {
  test.use({ allowErrors: CUT });
  test("an alarm, a dashboard and a kind that are not there say so", async ({ page }) => {
    await page.goto(`${server.console}${alarm("nope")}`);
    await expect(page.getByTestId("detail-error")).toContainText("There is no alarm named nope");
    await page.goto(`${server.console}${dash("nope")}`);
    await expect(page.getByTestId("detail-error")).toContainText("There is no dashboard named nope");
    await page.goto(`${server.console}/resources/cloudwatch/widgets`);
    await expect(page.getByText(/not described|no kind|not found/i).first()).toBeVisible();
  });
});

test("cloudwatch__dashboards-none and alarms-none: deleting the last one leaves the empty state", async ({ page }) => {
  await go(page, "/resources/cloudwatch/dashboards", "Dashboards");
  await page.getByTestId("select-all").click();
  await expect(page.getByTestId("selected-count")).toContainText("dashboard");
  await page.getByTestId("action-delete").click();
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("list-empty")).toContainText("No dashboards");
  await expect(page.getByTestId("list-empty")).toContainText("A dashboard is a saved arrangement of graphs and widgets.");
  await expect(page.getByTestId("empty-create")).toHaveText("Create dashboard");
  await shot(page, "cloudwatch__dashboards-none");
  await go(page, "/resources/cloudwatch/alarms", "Alarms");
  await page.getByTestId("select-all").click();
  await expect(page.getByTestId("selected-count")).toContainText("alarm");
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("alarms?");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("list-empty")).toContainText("No alarms");
  await expect(page.getByTestId("list-empty")).toContainText("An alarm watches one metric and changes state when it crosses a threshold.");
  await expect(page.getByTestId("empty-create")).toHaveText("Create alarm");
  await shot(page, "cloudwatch__alarms-none");
});
