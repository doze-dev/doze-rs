// The EventBridge boards of the event buses and the rules, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder"
// world with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR to be set beside the board. The flows run in the order the boards
// tell them: a bus is made and refused, rules are listed, events are sent, a rule is made through all five steps and deleted, a bus is deleted with its rules.
// The archives, replays and API destinations are eventbridge-boards-more.spec.ts.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { PATTERN, putEvent, seedLarder } from "./eventbridge-seed";
import { advance, expect, expectNoErrorAnswers, settled, test, pickFrom } from "./fixtures";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-eventbridge");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45400);
const at = (b: string) => BASE + ({ firefox: 0, webkit: 10, chromium: 20 }[b] ?? 30);

test.describe.configure({ mode: "serial" });

let server: Running;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

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
/** Chooses `option` in the combobox named `name`. */
const pick = async (page: Page, name: string | RegExp, option: string | RegExp) => {
  await pickFrom(page, name, option);
};
const events = () => {
  const put = (bus: string, source: string, type: string, detail: object) => expect(putEvent(server.endpoint, bus, source, type, detail).code).toBe(0);
  put("larder", "larder.orders", "OrderPlaced", { order_id: "o-20931", customer_id: "c-1187", total_cents: 4850 });
  put("larder", "larder.payments", "PaymentSettled", { order_id: "o-20931" });
};

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  seedLarder(server.endpoint, server.account);
  events();
});
test.afterAll(async () => {
  await server.stop();
});

/** The create-bus form of the boards: a customer managed key, a dead-letter queue, an archive and two tags. */
async function fillBus(page: Page, name: string) {
  await page.getByTestId("bus-name").fill(name);
  await page.getByTestId("enc-cmk").click();
  await pick(page, "Customer managed key", "alias/larder-app");
  await pick(page, /Choose a queue/, "pick-lists-dlq");
  await page.getByTestId("archive-on").click();
  const tags = page.getByTestId("form-tags");
  await tags.getByRole("textbox").nth(0).fill("Environment");
  await tags.getByRole("textbox").nth(1).fill("dev");
  await page.getByTestId("section-tags").getByRole("button", { name: "Add new tag" }).click();
  await tags.getByRole("textbox").nth(2).fill("Team");
  await tags.getByRole("textbox").nth(3).fill("platform");
}

test("eventbridge__bus-create: encryption, a dead-letter queue, an archive and tags, and the two calls it will make", async ({ page }) => {
  await go(page, "/resources/eventbridge/buses?create=yes", "Create event bus");
  await expect(page.getByTestId("section-details")).toContainText("larder already exists in this Region.");
  await fillBus(page, "larder-audit");
  await expect(page.getByTestId("field-kms")).toContainText("Key status: Enabled. Alias alias/larder-app.");
  await expect(page.getByTestId("section-encryption")).toContainText("Stored, not applied locally.");
  const call = page.getByTestId("call-panel").first();
  await expect(call).toContainText("CreateEventBus");
  await expect(call).toContainText('"KmsKeyIdentifier": "alias/larder-app"');
  await expect(call).toContainText("CreateArchive");
  await expect(call).toContainText("larder-audit-archive");
  await expect(call).toContainText("The archive is a separate call, made after the bus exists.");
  await shot(page, "eventbridge__bus-create");
  await advance(page);
  await expect(page.getByTestId("created-banner")).toContainText("Event bus larder-audit created.");
  await expect(page.getByTestId("bus-archives")).toContainText("larder-audit-archive");
  await expect(page.getByTestId("bus-details")).toContainText("alias/larder-app");
  await expect(page.getByTestId("bus-details")).toContainText("pick-lists-dlq");
  expect(cli(["events", "list-tags-for-resource", "--resource-arn", `arn:aws:events:us-east-1:${server.account}:event-bus/larder-audit`])).toContain("platform");
});

test("eventbridge__bus-create-refused: a name that exists is refused on the name, what was typed is kept, and CreateArchive is never sent", async ({ page, refusals }) => {
  // The name that exists is EventBridge's refusal, shown on the form.
  refusals.allow(/400 POST .*\/eventbridge\/buses\/op\/CreateEventBus$/);
  await go(page, "/resources/eventbridge/buses?create=yes", "Create event bus");
  await fillBus(page, "larder-audit");
  await advance(page);
  await expect(page.getByTestId("create-refused")).toContainText("CreateEventBus answered ResourceAlreadyExistsException");
  await expect(page.getByTestId("field-bus")).toHaveCount(0);
  await expect(page.getByTestId("section-details")).toContainText("A bus named larder-audit already exists. Pick another name.");
  await expect(page.getByTestId("section-details")).toContainText("larder-audit was created at");
  await expect(page.getByTestId("section-encryption-summary")).toContainText("Customer managed key alias/larder-app, with pick-lists-dlq as the dead-letter queue. Stored, not applied locally.");
  await expect(page.getByTestId("section-archives-summary")).toHaveText("Enabled, named larder-audit-archive.");
  await expect(page.getByTestId("section-tags-summary")).toHaveText("Environment=dev, Team=platform.");
  await expect(page.getByTestId("call-panel-came-back")).toContainText("ResourceAlreadyExistsException");
  await expect(page.getByTestId("call-panel")).toContainText("CreateArchive was never sent");
  // The panel is the board's: the call that was refused and what came back, not the call after it as well.
  await expect(page.getByTestId("call-panel-code")).toHaveCount(1);
  await expect(page.getByTestId("call-panel-code")).toContainText("CreateEventBus");
  await expect(page.getByTestId("call-panel-code")).not.toContainText("CreateArchive");
  await expect(page.getByTestId("bus-name")).toHaveValue("larder-audit");
  await shot(page, "eventbridge__bus-create-refused");
  // The sections have just folded and the page has scrolled to the name; a click that lands as it moves is not a click.
  await settled(page);
  await page.getByTestId("section-encryption-toggle").click();
  await expect(page.getByTestId("section-encryption")).toContainText("Use customer managed key");
});

test("eventbridge__buses-list: the default bus as a card, the custom buses with their ARNs, and the line over them", async ({ page }) => {
  await go(page, "/resources/eventbridge/buses", "Event buses");
  await expect(page.getByTestId("strip-replays")).toBeVisible();
  await expect(page.getByTestId("list-sentence")).toContainText("3 event buses.");
  await expect(page.getByTestId("list-sentence")).toContainText("larder has 3 rules and 3 archives");
  await expect(page.getByTestId("list-sentence")).toContainText("larder-audit has no rules and 1 archive");
  await expect(page.getByTestId("default-bus")).toContainText(`arn:aws:events:us-east-1:${server.account}:event-bus/default`);
  const table = page.getByRole("table", { name: "Custom event buses" });
  await expect(table.locator("tr[data-row]")).toHaveCount(2);
  await expect(table).toContainText(`arn:aws:events:us-east-1:${server.account}:event-bus/larder`);
  await expect(table).toContainText("Application events for the larder stack");
  await expect(page.getByText("This account has 3, counting default.")).toBeVisible();
  await page.getByTestId("list-search").fill("larder-a");
  await expect(table.locator("tr[data-row]")).toHaveCount(1);
  await page.getByTestId("list-search").fill("");
  await page.getByTestId("default-actions").click();
  await expect(page.getByRole("menuitem", { name: "Send events" })).toBeVisible();
  await shot(page, "eventbridge__buses-list");
  await page.keyboard.press("Escape");
});

test("eventbridge__rules-empty: a bus with no rules says so, and what its archive keeps", async ({ page }) => {
  await go(page, "/resources/eventbridge/rules?name=larder-audit", "Rules");
  await expect(page.getByTestId("list-sentence")).toContainText("The larder-audit bus has no rules. Events sent to it are accepted and then match nothing.");
  await expect(page.getByTestId("list-empty")).toContainText("No rules on larder-audit");
  await expect(page.getByTestId("list-empty")).toContainText("larder-audit-archive, with no filter, so it keeps every event it receives even with no rule.");
  await expect(page.getByTestId("chip-larder-audit")).toHaveAttribute("aria-checked", "true");
  await shot(page, "eventbridge__rules-empty");
});

test("eventbridge__rules-list: a bus at a time, the pattern and the targets in the row, rows chosen for Disable and Delete", async ({ page }) => {
  await go(page, "/resources/eventbridge/rules?name=larder", "Rules");
  await expect(page.getByTestId("list-sentence")).toContainText("The larder bus has 3 rules, all enabled, with 2 targets between them. order-placed sends to the SNS topic orders-created.");
  const table = page.getByRole("table", { name: "Rules" });
  await expect(table.locator("tr[data-row]")).toHaveCount(3);
  await expect(table.locator("tr[data-row]", { hasText: "order-placed" })).toContainText("larder.ordersOrderPlaced");
  await expect(table.locator("tr[data-row]", { hasText: "order-placed" })).toContainText("SNS topicorders-created");
  await expect(table.locator("tr[data-row]", { hasText: "payment-settled" })).toContainText("SQS queuepick-lists");
  await expect(table.locator("tr[data-row]", { hasText: "audit-sweep" })).toContainText("No targets");
  await expect(page.getByTestId("action-delete")).toBeDisabled();
  await page.getByTestId("select-order-placed").click();
  await expect(page.getByTestId("action-toggle")).toHaveText("Disable");
  await expect(page.getByText("Scheduled rules (legacy) run on the default bus.")).toBeVisible();
  await shot(page, "eventbridge__rules-list");
  // Disable is the rule's, made from the list; the row says so.
  await page.getByTestId("action-toggle").click();
  await expect(table.locator("tr[data-row]", { hasText: "order-placed" })).toContainText("Disabled");
  expect(cli(["events", "describe-rule", "--name", "order-placed", "--event-bus-name", "larder"])).toContain("DISABLED");
  await page.getByTestId("select-order-placed").click();
  await expect(page.getByTestId("action-toggle")).toHaveText("Enable");
  await page.getByTestId("action-toggle").click();
  await expect(table.locator("tr[data-row]", { hasText: "order-placed" })).toContainText("Enabled");
  await page.getByTestId("chip-default").click();
  await expect(table.locator("tr[data-row]")).toHaveCount(1);
  await expect(table).toContainText("cron(0 2 * * ? *)");
});

test("eventbridge__send-events: one PutEvents entry, which rules it matches, and the EventId that came back", async ({ page }) => {
  await go(page, "/resources/eventbridge/buses?do=send&name=default", "Send events");
  await expect(page.getByTestId("match-hint")).toContainText("The rule order-placed lives on larder, so this event on default matches none of your rules.");
  await expect(page.getByTestId("call-panel-code")).toContainText('"EventBusName": "default"');
  await shot(page, "eventbridge__send-events");
  await pick(page, /Choose an event bus/, "larder");
  await expect(page.getByTestId("match-hint")).toContainText("This event matches the rule order-placed on larder.");
  await expect(page.getByTestId("call-panel-code")).toContainText('"EventBusName": "larder"');
  // The match hint and the preview are still being redrawn; a click that lands as the page moves is not a click.
  await settled(page);
  await advance(page);
  await expect(page.getByTestId("sent-banner")).toContainText("PutEvents returned the EventId");
  await expect(page.getByTestId("sent-banner")).toContainText("It matched order-placed");
  // A detail that is not JSON is said on the field.
  await page.getByTestId("send-detail").fill("{");
  await expect(page.getByTestId("send-detail-problem")).toContainText("Not JSON");
  await expect(page.getByTestId("form-submit")).toBeDisabled();
  await page.getByTestId("form-cancel").click();
  await expect(page.locator("h1").first()).toHaveText("Event buses");
});

test("eventbridge__rule-detail: what feeds the rule, its pattern, its targets with the input and the retry policy, and what it delivered", async ({ page }) => {
  events();
  await go(page, `/resources/eventbridge/rules/${encodeURIComponent("larder/order-placed")}`, "order-placed");
  await expect(page.getByTestId("detail-sentence")).toContainText("order-placed matches events from larder.orders with detail type OrderPlaced and a total_cents above 0, and publishes it to the orders-created topic.");
  await expect(page.getByTestId("wired-strip").first()).toContainText("larder bus");
  // What the topic fans out to, past the rule.
  await expect(page.getByTestId("wired-strip").first()).toContainText("3 subscribed queues");
  await expect(page.getByTestId("rule-details")).toContainText("Rule with an event pattern");
  await expect(page.getByTestId("rule-details")).toContainText("Team=checkout");
  await expect(page.getByTestId("pattern-box")).toContainText('"source": ["larder.orders"]');
  await expect(page.getByTestId("pattern-box")).toContainText('"total_cents": [{ "numeric": [">", 0] }]');
  const targets = page.getByRole("table", { name: "Targets" });
  await expect(targets).toContainText("publish-orders-created");
  await expect(targets).toContainText("Input transformer order_id, customer");
  // The whole ARN is on the page (it wraps), as is the delivery's input.
  await expect(page.getByTestId("target-arn")).toHaveText(`arn:aws:sns:us-east-1:${server.account}:orders-created`);
  await expect(targets).toContainText("1 hour up to 20 attempts");
  await expect(targets).toContainText("pick-lists-dlq");
  await expect(page.getByTestId("rule-targets")).toContainText("Applied here:");
  const calls = page.getByTestId("recent-calls");
  await expect(calls).toContainText("EventBridge rule order-placed");
  await expect(calls).toContainText("Publish");
  await expect(calls).toContainText("to orders-created, input transformer, 2 variables");
  // One row per delivery: the topic's policy the rule read first is not a row of its own.
  await expect(calls).not.toContainText("GetTopicAttributes");
  await shot(page, "eventbridge__rule-detail");
});

test("eventbridge__rule-targets-none: a rule with no target says what it does, and a target is added from there", async ({ page }) => {
  await go(page, `/resources/eventbridge/rules/${encodeURIComponent("larder/audit-sweep")}`, "audit-sweep");
  await expect(page.getByTestId("empty-tab")).toContainText("No targets");
  await expect(page.getByTestId("empty-tab")).toContainText("audit-sweep matches events but has nowhere to send them. A rule needs at least one target to do anything.");
  await shot(page, "eventbridge__rule-targets-none");
  await page.getByTestId("add-target").click();
  await expect(page.locator("h1").first()).toHaveText("Add target");
  await pick(page, /Select a target/, "SQS queue");
  await pick(page, /Choose a queue/, "analytics-queue");
  await expect(page.getByTestId("call-panel-code")).toContainText('"Id": "send-analytics-queue"');
  await advance(page);
  await expect(page.getByTestId("target-banner")).toContainText("Target added.");
  await expect(page.getByRole("table", { name: "Targets" })).toContainText("send-analytics-queue");
  cli(["events", "remove-targets", "--rule", "audit-sweep", "--event-bus-name", "larder", "--ids", "send-analytics-queue"]);
});

test("eventbridge__bus-detail: what feeds the bus and what its rules send to, its details, archives and the calls that touched it", async ({ page }) => {
  events();
  await go(page, "/resources/eventbridge/buses/larder", "larder");
  await expect(page.getByTestId("detail-sentence")).toContainText("larder has 3 rules and 3 archives. Events sent to this bus by name reach only its rules, never the ones on default.");
  await expect(page.getByText("Custom event bus")).toBeVisible();
  const strips = page.getByTestId("wired-strip");
  await expect(strips.first()).toContainText("Fed by");
  await expect(strips.nth(1)).toContainText("order-placed");
  // Each rule is read with its own target (not all the rules, then all the targets).
  await expect(strips.nth(1)).toHaveText(/order-placed\s*SNS topic orders-created/);
  await expect(page.getByRole("heading", { name: "Event bus details" })).toBeVisible();
  await expect(strips.nth(1)).toContainText("SNS topic orders-created");
  await expect(page.getByTestId("bus-details")).toContainText("AWS owned key");
  await expect(page.getByTestId("bus-details")).toContainText("Environment=dev");
  await expect(page.getByRole("table", { name: "Archives" })).toContainText("30 days");
  await expect(page.getByTestId("bus-archives")).toContainText("Retention is applied.");
  const calls = page.getByTestId("recent-calls");
  await expect(calls).toContainText("PutEvents");
  await expect(calls).toContainText("matched order-placed");
  // The call says how many events it carried; the match is read from the deliveries that followed it.
  await expect(calls.getByTestId("answer-words").filter({ hasText: /^· \d+ events?$/ }).first()).toBeVisible();
  await shot(page, "eventbridge__bus-detail");
});

test("eventbridge__bus-delete: the rules that go with the bus are named, and an app that keeps sending is refused", async ({ page, refusals }) => {
  for (const r of ["a-rule", "b-rule"]) cli(["events", "put-rule", "--name", r, "--event-bus-name", "larder-audit", "--event-pattern", JSON.stringify({ source: ["x"] })]);
  cli(["events", "put-targets", "--rule", "a-rule", "--event-bus-name", "larder-audit", "--targets", JSON.stringify([{ Id: "t", Arn: `arn:aws:sqs:us-east-1:${server.account}:pick-lists` }])]);
  await go(page, "/resources/eventbridge/buses/larder-audit", "larder-audit");
  await page.getByTestId("verb-delete").click();
  const dialog = page.getByTestId("bus-delete");
  await expect(dialog).toContainText("Delete event bus?");
  await expect(dialog).toContainText("Deleting an event bus deletes the rules associated with that event bus: a-rule and b-rule");
  await expect(dialog).toContainText("never the default one");
  await expect(dialog).toContainText("gets ResourceNotFoundException from PutEvents: Event bus larder-audit does not exist.");
  // The archive that takes this bus's events stays, and the dialog says so.
  await expect(dialog.getByTestId("bus-delete-archives")).toContainText("larder-audit-archive");
  await expect(dialog).toContainText("an archive is its own resource, and DeleteEventBus does not touch it.");
  await shot(page, "eventbridge__bus-delete");
  await page.getByTestId("bus-delete-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Event bus deleted.");
  await expectNoErrorAnswers(page, refusals);
  expect(cli(["events", "list-event-buses"])).not.toContain("larder-audit");
  expect(putEvent(server.endpoint, "larder-audit", "x", "y", {}).out).toContain("ResourceNotFoundException");
  // The archive of the bus is not part of the bus: it is still listed.
  expect(cli(["events", "list-archives"])).toContain("larder-audit-archive");
});

void PATTERN;
