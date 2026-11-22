// The EventBridge boards of making a rule and deleting it, against a real doze-aws seeded as the boards' "larder" world: the five steps of Create rule (the
// definition, the pattern and its test, the target with its input transformer, retry policy and dead-letter queue, the tags, the review), the scheduled
// rule, and Delete rule with the two calls it makes. In Chromium each board is photographed light and dark into SHOTS_DIR.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { seedLarder } from "./eventbridge-seed";
import { expect, expectNoErrorAnswers, test } from "./fixtures";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-eventbridge");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45424);
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
const pick = async (page: Page, name: string | RegExp, option: string | RegExp) => {
  await page.getByRole("combobox", { name }).click();
  await page.getByRole("option", { name: option, exact: typeof option === "string" }).click();
};
const next = (page: Page) => page.getByTestId("form-submit").click();

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  seedLarder(server.endpoint, server.account);
});
test.afterAll(async () => {
  await server.stop();
});

test("eventbridge__rule-create: Create rule's five steps, each reached in turn, and the rule they make", async ({ page }) => {
  await go(page, "/resources/eventbridge/rules?create=yes&name=larder", "Create rule");
  await expect(page.getByText("Step 1 of 5, Advanced Builder.")).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Steps" })).toContainText("Define rule detail");
  await page.getByTestId("rule-name").fill("order-placed");
  await expect(page.getByTestId("section-detail")).toContainText("audit-sweep and order-placed are already on larder, with 1 more.");
  await next(page);
  await expect(page.getByTestId("section-detail")).toContainText("order-placed is already on larder.");
  await page.getByTestId("rule-name").fill("refund-issued");
  await page.getByTestId("rule-description").fill("Tell finance about each refund");
  await expect(page.getByTestId("field-bus")).toContainText("larder");
  await expect(page.getByTestId("legacy-note")).toContainText("Scheduled rule (legacy)");
  await shot(page, "eventbridge__rule-create");
  await next(page);

  // Step 2: the pattern, a sample event, and the test of one against the other.
  await expect(page.locator("h1").first()).toHaveText("Build event pattern");
  await expect(page.getByText("Step 2 of 5.")).toBeVisible();
  await page.getByTestId("event-pattern").fill('{\n  "source": ["larder.orders"],\n  "detail-type": ["OrderPlaced"],\n  "detail": {\n    "total_cents": [{ "numeric": [">", 0] }]\n  }\n}');
  await page.getByTestId("test-pattern").click();
  await expect(page.getByTestId("test-result")).toHaveAttribute("data-match", "yes");
  await expect(page.getByTestId("test-result")).toContainText("The sample event matches the event pattern. It checked source, detail-type, detail.total_cents");
  await shot(page, "eventbridge__rule-create-pattern");
  await page.getByTestId("event-pattern").fill('{ "source": ["larder.payments"] }');
  await page.getByTestId("test-pattern").click();
  await expect(page.getByTestId("test-result")).toHaveAttribute("data-match", "no");
  await expect(page.getByTestId("test-result")).toContainText("does not match");
  await page.getByTestId("pattern-form-open").click();
  await page.getByTestId("form-source").fill("larder.orders");
  await page.getByTestId("form-detail-type").fill("OrderPlaced");
  await page.getByTestId("form-apply").click();
  await expect(page.getByTestId("event-pattern")).toHaveValue(/"detail-type": \[\s*"OrderPlaced"\s*\]/);
  await page.getByTestId("test-pattern").click();
  await expect(page.getByTestId("test-result")).toHaveAttribute("data-match", "yes");
  await page.getByTestId("event-pattern").fill("{");
  await expect(page.getByTestId("event-pattern-problem")).toContainText("Not JSON");
  await page.getByTestId("event-pattern").fill('{\n  "source": ["larder.orders"],\n  "detail-type": ["OrderPlaced"]\n}');
  await next(page);

  // Step 3: the target, its input, its retry policy and its dead-letter queue.
  await expect(page.locator("h1").first()).toHaveText("Select target(s)");
  await next(page);
  await expect(page.getByTestId("field-where-1")).toContainText("Choose where the target sends.");
  await pick(page, /Choose a topic/, "orders-created");
  await expect(page.getByTestId("arn-1")).toContainText(`arn:aws:sns:us-east-1:${server.account}:orders-created`);
  await expect(page.getByTestId("fanout-1")).toContainText("orders-created already fans out to");
  await page.getByTestId("input-0-transformer").click();
  await page.getByTestId("transformer-open-1").click();
  const dialog = page.getByTestId("transformer-dialog");
  await dialog.getByTestId("transformer-generate").click();
  await expect(dialog.getByTestId("transformer-output")).toContainText('"order_id": "o-20931"');
  await expect(dialog.getByTestId("transformer-output")).toContainText('"customer": "c-1187"');
  await shot(page, "eventbridge__rule-transformer");
  await dialog.getByTestId("transformer-template").fill('{"order_id": "<order_id>", "who": "<customer>", "missing": "<nope>"}');
  await dialog.getByTestId("transformer-generate").click();
  await expect(dialog.getByTestId("transformer-problem")).toContainText("The template names <nope>, which no input path defines.");
  await dialog.getByTestId("transformer-template").fill('{\n  "order_id": "<order_id>",\n  "customer": "<customer>"\n}');
  await dialog.getByTestId("transformer-confirm").click();
  await expect(page.getByTestId("transformer-set-1")).toContainText("Transformer set: 2 variables, order_id and customer, shape the message published to orders-created.");
  await page.getByTestId("age-hours-1").fill("1");
  await expect(page.getByTestId("age-hours-1")).toHaveValue("1");
  await page.getByTestId("age-minutes-1").fill("0");
  await page.getByTestId("age-seconds-1").fill("0");
  await page.getByTestId("attempts-1").fill("20");
  await expect(page.getByTestId("age-hours-1")).toHaveValue("1");
  await page.getByTestId("dlq1-queue").click();
  await pick(page, "Dead-letter queue", "pick-lists-dlq");
  await expect(page.getByTestId("section-settings-1")).toContainText("Applied here:");
  await page.getByTestId("add-target").click();
  await expect(page.getByTestId("section-target-2")).toBeVisible();
  await page.getByTestId("remove-target-2").click();
  await expect(page.getByTestId("section-target-2")).toHaveCount(0);
  await shot(page, "eventbridge__rule-create-targets");
  await next(page);

  // Step 4 and 5: the tags, and the review with its calls.
  await expect(page.locator("h1").first()).toHaveText("Configure tags");
  const tags = page.getByTestId("form-tags");
  await tags.getByRole("textbox").nth(0).fill("Environment");
  await tags.getByRole("textbox").nth(1).fill("dev");
  await page.getByTestId("section-tags").getByRole("button", { name: "Add new tag" }).click();
  await tags.getByRole("textbox").nth(2).fill("Team");
  await tags.getByRole("textbox").nth(3).fill("checkout");
  await expect(page.getByTestId("section-tags")).toContainText("The bus larder is tagged Environment=dev and Team=platform too.");
  await shot(page, "eventbridge__rule-create-tags");
  await next(page);
  await expect(page.locator("h1").first()).toHaveText("Review and create");
  await expect(page.getByTestId("rule-create")).toContainText("Step 1: Define rule detail");
  await expect(page.getByTestId("review-pattern")).toContainText('"source": ["larder.orders"]');
  // Printed over several lines, as the board prints it, not as one compact line.
  expect(((await page.getByTestId("review-pattern").textContent()) ?? "").includes("\n")).toBe(true);
  await expect(page.getByTestId("rule-create")).toContainText("SNS topic orders-created");
  await expect(page.getByTestId("rule-create")).toContainText("Input transformer order_id, customer");
  await expect(page.getByTestId("rule-create")).toContainText("1 hour up to 20 attempts");
  const call = page.getByTestId("call-panel").first();
  await expect(call).toContainText("PutRule");
  await expect(call).toContainText('"EventBusName": "larder"');
  await expect(call).toContainText("PutTargets");
  await expect(call).toContainText('"MaximumRetryAttempts": 20');
  await shot(page, "eventbridge__rule-create-review");
  // Edit goes back to the step; Create makes PutRule, then PutTargets.
  await page.getByTestId("review-edit-Step 1: Define rule detail").click();
  await expect(page.getByTestId("rule-name")).toHaveValue("refund-issued");
  for (let i = 0; i < 4; i++) await next(page);
  await expect(page.locator("h1").first()).toHaveText("Review and create");
  await next(page);
  await expect(page.getByTestId("created-banner")).toContainText("Rule refund-issued created.");
  await expect(page.getByRole("table", { name: "Targets" })).toContainText("publish-orders-created");
  await expect(page.getByRole("table", { name: "Targets" })).toContainText("Input transformer order_id, customer");
  expect(cli(["events", "describe-rule", "--name", "refund-issued", "--event-bus-name", "larder"])).toContain("ENABLED");
  expect(cli(["events", "list-tags-for-resource", "--resource-arn", `arn:aws:events:us-east-1:${server.account}:rule/larder/refund-issued`])).toContain("checkout");
});

test("eventbridge__rule-create-schedule: a scheduled rule on the default bus, its cron in six fields, in words, and its next ten dates", async ({ page }) => {
  await go(page, "/resources/eventbridge/rules?do=schedule", "Create scheduled rule");
  await expect(page.getByText("Legacy", { exact: true })).toBeVisible();
  await expect(page.getByTestId("section-detail")).toContainText("Scheduled rules can only be created on the default event bus.");
  await expect(page.getByTestId("section-detail")).toContainText("Enable the scheduled rule");
  await page.getByTestId("rule-name").fill("nightly-report");
  await expect(page.getByTestId("section-detail")).toContainText("nightly-export is already on default.");
  await page.getByTestId("schedule-mode-cron").click();
  await page.getByTestId("cron-minutes").fill("0");
  await page.getByTestId("cron-hours").fill("18");
  await page.getByTestId("cron-dom").fill("?");
  await page.getByTestId("cron-month").fill("*");
  await page.getByTestId("cron-dow").fill("MON-FRI");
  await page.getByTestId("cron-year").fill("*");
  await expect(page.getByTestId("cron-words")).toContainText("This is cron(0 18 ? * MON-FRI *): 6:00 pm UTC every Monday through Friday.");
  await expect(page.getByTestId("run-list").locator("li")).toHaveCount(10);
  await expect(page.getByTestId("run-list").locator("li").first()).toContainText("18:00 UTC");
  await page.getByTestId("zone-local").click();
  await expect(page.getByTestId("run-list").locator("li")).toHaveCount(10);
  await page.getByTestId("zone-utc").click();
  await shot(page, "eventbridge__rule-create-schedule");
  // Day of month and day of week can't both have a value.
  await page.getByTestId("cron-dom").fill("1");
  await expect(page.getByTestId("schedule-problem")).toContainText("Day of month and day of week can't both have a value: put ? in one.");
  await page.getByTestId("cron-dom").fill("?");
  // The rate form, and what it refuses.
  await page.getByTestId("schedule-mode-rate").click();
  await page.getByTestId("rate-expression").fill("rate(1 hours)");
  await next(page);
  await expect(page.getByTestId("section-schedule")).toContainText("The unit is singular for 1: rate(1 hour).");
  await page.getByTestId("rate-expression").fill("rate(5 minutes)");
  await next(page);
  await expect(page.locator("h1").first()).toHaveText("Select target(s)");
  await pick(page, /Select a target/, "SQS queue");
  await pick(page, /Choose a queue/, "pick-lists");
  await next(page);
  await next(page);
  await expect(page.locator("h1").first()).toHaveText("Review and create");
  await expect(page.getByTestId("call-panel").first()).toContainText('"ScheduleExpression": "rate(5 minutes)"');
  await next(page);
  await expect(page.getByTestId("created-banner")).toContainText("Rule nightly-report created.");
  await expect(page.getByTestId("rule-details")).toContainText("Scheduled rule (legacy)");
  expect(cli(["events", "describe-rule", "--name", "nightly-report"])).toContain("rate(5 minutes)");
});

test("eventbridge__rule-delete: RemoveTargets, then DeleteRule, each as the call it is, and the target's resource is not touched", async ({ page, refusals }) => {
  await go(page, "/resources/eventbridge/rules?name=larder", "Rules");
  await page.getByTestId("select-refund-issued").click();
  await page.getByTestId("action-delete").click();
  const dialog = page.getByTestId("rule-delete");
  await expect(dialog).toContainText("Delete rule?");
  await expect(dialog).toContainText("Delete refund-issued from the larder bus?");
  await expect(dialog.getByTestId("delete-calls")).toContainText("RemoveTargets");
  await expect(dialog.getByTestId("delete-calls")).toContainText('"Ids"');
  await expect(dialog.getByTestId("delete-calls")).toContainText("DeleteRule");
  await expect(dialog.getByTestId("delete-calls")).toContainText("The API reference says a rule's targets must be removed before the rule can be deleted, which is why the calls go in this order. orders-created is not touched.");
  await shot(page, "eventbridge__rule-delete");
  await page.getByTestId("rule-delete-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Rule deleted.");
  await expectNoErrorAnswers(page, refusals);
  expect(cli(["events", "list-rules", "--event-bus-name", "larder"])).not.toContain("refund-issued");
  expect(cli(["sns", "list-topics"])).toContain("orders-created");
  // The rule's own page deletes it the same way.
  cli(["events", "put-rule", "--name", "scratch", "--event-bus-name", "larder", "--event-pattern", JSON.stringify({ source: ["x"] })]);
  cli(["events", "put-targets", "--rule", "scratch", "--event-bus-name", "larder", "--targets", JSON.stringify([{ Id: "t", Arn: `arn:aws:sqs:us-east-1:${server.account}:pick-lists` }])]);
  await go(page, `/resources/eventbridge/rules/${encodeURIComponent("larder/scratch")}`, "scratch");
  await page.getByTestId("verb-delete").click();
  await page.getByTestId("rule-delete-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Rule deleted.");
  await expectNoErrorAnswers(page, refusals);
  expect(cli(["events", "list-rules", "--event-bus-name", "larder"])).not.toContain("scratch");
});
