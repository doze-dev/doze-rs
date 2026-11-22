// The CloudWatch boards of the metrics and the dashboards (browse, graph, add to a dashboard, the dashboards, their widgets, the not-found
// pages): each reached against a real doze-aws and, in Chromium, photographed light and dark into SHOTS_DIR. No test may log a console
// error; a not-found page is the one refusal, and says so.
import { expect, expectNoErrorAnswers, test } from "./fixtures";
import { alarm, cli, dash, go, shot, serveWorld, world } from "./cloudwatch-kit";

serveWorld(1);

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


test("cloudwatch__dashboards-list, dashboard-create and dashboard-delete", async ({ page, refusals }) => {
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
  await expectNoErrorAnswers(page, refusals);
  await expect(table.locator("tr[data-row]", { hasText: "scratch-board" })).toHaveCount(0);
  // Deleted from its own page, the dashboard is not asked for again (a read of it would be refused and logged).
  cli(["cloudwatch", "put-dashboard", "--dashboard-name", "scratch-two", "--dashboard-body", '{"widgets":[]}']);
  await go(page, dash("scratch-two"), "scratch-two");
  await page.getByTestId("delete-open").click();
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Dashboard scratch-two deleted.");
  await expectNoErrorAnswers(page, refusals);
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
  test("an alarm, a dashboard and a kind that are not there say so", async ({ page, refusals }) => {
    // A page of what is not there asks the service, which refuses.
    refusals.allow(/(404|400) GET .*\/cloudwatch\/(alarms|dashboards)\/(nope|widgets)$/);
    await page.goto(`${world.server.console}${alarm("nope")}`);
    await expect(page.getByTestId("detail-error")).toContainText("There is no alarm named nope");
    await page.goto(`${world.server.console}${dash("nope")}`);
    await expect(page.getByTestId("detail-error")).toContainText("There is no dashboard named nope");
    await page.goto(`${world.server.console}/resources/cloudwatch/widgets`);
    await expect(page.getByText(/not described|no kind|not found/i).first()).toBeVisible();
  });
});


test("cloudwatch__dashboards-none: deleting the last dashboard leaves the empty state", async ({ page, refusals }) => {
  await go(page, "/resources/cloudwatch/dashboards", "Dashboards");
  await page.getByTestId("select-all").click();
  await expect(page.getByTestId("selected-count")).toContainText("dashboard");
  await page.getByTestId("action-delete").click();
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("list-empty")).toContainText("No dashboards");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("list-empty")).toContainText("A dashboard is a saved arrangement of graphs and widgets.");
  await expect(page.getByTestId("empty-create")).toHaveText("Create dashboard");
  await shot(page, "cloudwatch__dashboards-none");
});
