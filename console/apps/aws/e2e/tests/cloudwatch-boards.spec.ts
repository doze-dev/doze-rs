// The CloudWatch boards of the alarms (overview, list, delete, detail, history, tags, the four steps of the wizard, and none left): each reached against a real doze-aws and, in Chromium, photographed light and dark into SHOTS_DIR.
// No test may log a console error.
import { advance, press, expect, expectNoErrorAnswers, test } from "./fixtures";
import { alarm, choose, cli, code, go, pick, shot, serveWorld, world } from "./cloudwatch-kit";

serveWorld(0);

test("cloudwatch__overview: alarms by AWS service, the recent alarms, and where to go", async ({ page }) => {
  await go(page, "/resources/cloudwatch", "CloudWatch");
  await expect(page.getByTestId("overview-lead")).toContainText(`account ${world.server.account} in us-east-1`);
  await expect(page.getByTestId("overview-lead")).toContainText("Larder/Orders and Larder/Queues");
  const service = page.getByRole("table", { name: "Alarms by AWS service" });
  await expect(service.locator("tr[data-row]", { hasText: "AWS Lambda" })).toContainText("1");
  await expect(service.locator("tr[data-row]", { hasText: "Amazon API Gateway" })).toContainText("1");
  const recent = page.getByRole("table", { name: "Recent alarms" });
  await expect(recent.locator("tr[data-row]").first()).toContainText("dlq-not-empty");
  await expect(recent.locator("tr[data-row]").first()).toContainText("ALARM");
  // Three rows, as the board draws: the alarms that changed most recently, the oldest left to the Alarms page.
  await expect(recent.locator("tr[data-row]")).toHaveCount(3);
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


test("cloudwatch__alarm-delete: the dialog says what stops, then deleting leaves the list without it", async ({ page, refusals }) => {
  cli(["cloudwatch", "put-metric-alarm", "--alarm-name", "scratch-alarm", "--namespace", "Larder/Queues", "--metric-name", "DlqMessages", "--dimensions", "Name=QueueName,Value=pick-lists-dlq", "--statistic", "Maximum", "--period", "60", "--evaluation-periods", "1", "--threshold", "1", "--comparison-operator", "GreaterThanOrEqualToThreshold", "--alarm-actions", `arn:aws:sns:us-east-1:${world.server.account}:alerts`]);
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
  await expectNoErrorAnswers(page, refusals);
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
  // The threshold's label is drawn above its line, in the band that is bad, with room over the line (the data peaks at the threshold).
  await expect(page.getByTestId("alarm-graph-unit")).toHaveText("Count");
  const band = page.getByTestId("alarm-graph-threshold");
  const label = await band.locator("text").boundingBox();
  const line = await band.locator("line").boundingBox();
  const plot = await page.getByTestId("alarm-graph").locator("svg").boundingBox();
  expect(label && line && plot).toBeTruthy();
  expect(label!.y + label!.height).toBeLessThanOrEqual(line!.y + 2);
  expect(line!.y - plot!.y).toBeGreaterThan(30);
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
  await expect(page.getByTestId("alarm-arn")).toContainText(`arn:aws:cloudwatch:us-east-1:${world.server.account}:alarm:dlq-not-empty`);
  await expect(page.getByTestId("actions-enabled")).toContainText("Yes");
  await expect(page.getByTestId("actions-alarm")).toContainText("Send a notification to alerts");
  await expect(page.getByTestId("actions-alarm")).toContainText(`arn:aws:sns:us-east-1:${world.server.account}:alerts`);
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
  await expect(table).toContainText(`Successfully executed action arn:aws:sns:us-east-1:${world.server.account}:alerts`);
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
  expect(cli(["cloudwatch", "list-tags-for-resource", "--resource-arn", `arn:aws:cloudwatch:us-east-1:${world.server.account}:alarm:dlq-not-empty`])).toContain("kitchen");
});


test("cloudwatch__alarm-step1 to step4: the wizard, the call each step makes, and the alarm it creates", async ({ page }) => {
  await go(page, "/resources/cloudwatch/alarms", "Alarms");
  await page.getByTestId("create-open").click();
  await expect(page.locator("h1").first()).toHaveText("Create alarm");
  await expect(page.getByText("Step 1 of 4. Choose the metric to watch and the condition that puts the alarm in ALARM.")).toBeVisible();
  for (const s of ["Specify metric and conditions", "Configure actions", "Add name and description", "Preview and create"]) await expect(page.getByTestId("alarm-wizard").getByText(s).first()).toBeVisible();
  await press(page);
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
  await advance(page);
  await expect(page.getByText("Step 2 of 4. Choose what happens when the alarm changes state.")).toBeVisible();
  await expect(page.getByTestId("section-notification")).toContainText("Send a message to an SNS topic when the alarm enters a state.");
  await advance(page);
  await expect(page.getByText("Choose the topic to notify.").first()).toBeVisible();
  await page.getByRole("combobox", { name: "Topic" }).click();
  await page.getByRole("option", { name: "alerts" }).click();
  await expect(page.getByTestId("topic-arn")).toHaveText(`arn:aws:sns:us-east-1:${world.server.account}:alerts`);
  await expect(page.getByTestId("topic-note")).toContainText("CloudWatch does not test the topic.");
  await expect(page.getByTestId("topic-note")).toContainText("alerts delivers to the Lambda function send-email");
  await expect(code(page)).toContainText(`arn:aws:sns:us-east-1:${world.server.account}:alerts`);
  await expect(code(page)).toContainText('"ActionsEnabled": true');
  await expect(page.getByRole("button", { name: "Add Lambda action" })).toBeEnabled();
  for (const b of ["Auto Scaling action", "EC2 action", "Add Systems Manager action", "Add investigation action"]) await expect(page.getByRole("button", { name: b })).toBeDisabled();
  await expect(page.getByTestId("actions-off-reason")).toContainText("SNS topics and Lambda functions only");
  await shot(page, "cloudwatch__alarm-step2");
  await advance(page);
  await expect(page.getByText("Step 3 of 4. Name the alarm and say what it means.")).toBeVisible();
  await advance(page);
  await expect(page.getByText("Enter the alarm name.")).toBeVisible();
  await page.getByTestId("alarm-name").fill("dlq-not-empty");
  await advance(page);
  await expect(page.getByText("An alarm named dlq-not-empty already exists in this Region.")).toBeVisible();
  await page.getByTestId("alarm-name").fill("pick-lists-dlq-watch");
  await page.getByTestId("alarm-description").fill("The dead-letter queue pick-lists-dlq holds a message. Look at the message first.");
  await expect(code(page)).toContainText('"AlarmName": "pick-lists-dlq-watch"');
  await shot(page, "cloudwatch__alarm-step3");
  await advance(page);
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
  await advance(page);
  await advance(page);
  await expect(page.locator("h1").first()).toHaveText("pick-lists-dlq-watch");
  await expect(page.getByTestId("created-banner")).toContainText("Alarm pick-lists-dlq-watch created.");
  expect(cli(["cloudwatch", "describe-alarms", "--alarm-names", "pick-lists-dlq-watch"])).toContain("Look at the message first.");
  // Edit: the same pages over the alarm, its name kept.
  await page.getByTestId("actions-menu").click();
  await page.getByRole("menuitem", { name: "Edit" }).click();
  await expect(page.locator("h1").first()).toHaveText("Edit alarm");
  await advance(page);
  // The topic the alarm notifies opens on the existing-topic choice with the topic chosen, not on a typed ARN.
  await expect(page.getByTestId("via-existing")).toBeChecked();
  await expect(page.getByTestId("topic-arn")).toContainText(":alerts");
  await advance(page);
  await expect(page.getByTestId("alarm-name")).toBeDisabled();
  await advance(page);
  await advance(page);
  await expect(page.getByTestId("updated-banner")).toContainText("updated");
});


test("what the boards do not draw: an anomaly alarm from the wizard, a metric math alarm read back, and CloudWatch-Default on the overview", async ({ page, refusals }) => {
  // A band worked out from the metric's own data in place of a threshold.
  await go(page, "/resources/cloudwatch/alarms", "Alarms");
  await page.getByTestId("create-open").click();
  await choose(page, "Namespace", "Larder/Orders");
  await choose(page, "Metric name", "OrdersPlaced");
  await choose(page, "Dimensions", "Environment=prod");
  await page.getByTestId("kind-anomaly").click();
  await expect(page.getByTestId("math-toggle")).toBeDisabled();
  await page.getByTestId("band").fill("3");
  await expect(code(page)).toContainText('"ThresholdMetricId": "ad1"');
  await expect(code(page)).toContainText("ANOMALY_DETECTION_BAND(m1, 3)");
  await expect(code(page)).toContainText("LessThanLowerOrGreaterThanUpperThreshold");
  await advance(page);
  await page.getByTestId("notification-remove").click();
  await advance(page);
  await page.getByTestId("alarm-name").fill("orders-band");
  await advance(page);
  await advance(page);
  await expect(page.getByTestId("created-banner")).toContainText("orders-band");
  await expect(page.getByTestId("alarm-threshold")).toContainText("OrdersPlaced is outside the band");
  // Deleted from its own page, the alarm is not asked for again (a read of it would be refused and logged).
  await expect(page.getByTestId("rail-recent")).toContainText("orders-band");
  await page.getByTestId("actions-menu").click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Alarm orders-band deleted.");
  // It was open a moment ago, so the rail had it under Recent: a deleted alarm is not somewhere to go back to.
  await expect(page.getByTestId("rail-recent").filter({ hasText: "orders-band" })).toHaveCount(0);
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByRole("table", { name: "Alarms" }).locator("tr[data-row]", { hasText: "orders-band" })).toHaveCount(0);
  // Metric math, made with the CLI: the page names the expression and draws the one it returns.
  cli(["cloudwatch", "put-metric-alarm", "--alarm-name", "failure-rate", "--metrics", JSON.stringify([{ Id: "m1", MetricStat: { Metric: { Namespace: "Larder/Orders", MetricName: "OrdersPlaced", Dimensions: [{ Name: "Environment", Value: "prod" }] }, Period: 300, Stat: "Sum" }, ReturnData: false }, { Id: "m2", MetricStat: { Metric: { Namespace: "Larder/Orders", MetricName: "OrdersFailed", Dimensions: [{ Name: "Environment", Value: "prod" }] }, Period: 300, Stat: "Sum" }, ReturnData: false }, { Id: "e1", Expression: "m2 / m1 * 100", Label: "Failure rate", ReturnData: true }]), "--evaluation-periods", "1", "--threshold", "10", "--comparison-operator", "GreaterThanThreshold"]);
  await go(page, alarm("failure-rate"), "failure-rate");
  await expect(page.getByTestId("alarm-metric")).toContainText("Metric math");
  await expect(page.getByTestId("alarm-metric")).toContainText("e1: m2 / m1 * 100");
  await expect(page.getByTestId("alarm-threshold")).toContainText("Failure rate > 10 for 1 datapoints within 5 minutes");
  await expect(page.getByTestId("alarm-chart")).toContainText("Failure rate");
  // The dashboard named CloudWatch-Default is drawn on the overview.
  cli(["cloudwatch", "put-dashboard", "--dashboard-name", "CloudWatch-Default", "--dashboard-body", JSON.stringify({ widgets: [{ type: "text", x: 0, y: 0, width: 12, height: 3, properties: { markdown: "# Kitchen at a glance" } }] })]);
  await go(page, "/resources/cloudwatch", "CloudWatch");
  await expect(page.getByTestId("default-dashboard")).toContainText("Kitchen at a glance");
});


test("cloudwatch__alarms-none: deleting the last alarm leaves the empty state", async ({ page, refusals }) => {
  await go(page, "/resources/cloudwatch/alarms", "Alarms");
  await page.getByTestId("select-all").click();
  await expect(page.getByTestId("selected-count")).toContainText("alarm");
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("alarms?");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("list-empty")).toContainText("No alarms");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("list-empty")).toContainText("An alarm watches one metric and changes state when it crosses a threshold.");
  await expect(page.getByTestId("empty-create")).toHaveText("Create alarm");
  await shot(page, "cloudwatch__alarms-none");
});
