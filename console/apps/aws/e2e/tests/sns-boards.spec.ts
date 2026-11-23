// The 32 SNS boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder" world with the real
// AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board. The flows run in the order the boards
// tell them: a FIFO topic is made, subscribed, published to; the webhook is a real local server whose SubscribeURL is pasted to confirm; the topics are
// deleted last, which leaves the empty lists.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { queueArn, seedLarder, startWebhook, topicArn, type Webhook } from "./sns-seed";
import { expect, expectNoErrorAnswers, test } from "./fixtures";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-sns");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45200);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });

let server: Running;
let hook: Webhook;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};
const ARN = (name: string) => topicArn(server.account, name);
const FIFO = "orders-events.fifo";

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
const topicPath = (name: string, q = "") => `/resources/sns/topics/${encodeURIComponent(name)}${q}`;
const subs = (topic: string): { SubscriptionArn: string; Protocol: string; Endpoint: string }[] => (JSON.parse(cli(["sns", "list-subscriptions-by-topic", "--topic-arn", ARN(topic)])) as { Subscriptions: { SubscriptionArn: string; Protocol: string; Endpoint: string }[] }).Subscriptions;

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  hook = startWebhook(at(browserName) + 2);
  seedLarder(server.endpoint, server.account);
});
test.afterAll(async () => {
  hook.stop();
  await server.stop();
});

test("sns__topic-create: every setting of a topic, the sentence, and the call it will make", async ({ page }) => {
  await go(page, "/resources/sns/topics?create=yes", "Create topic");
  await page.getByTestId("topic-name").fill("deliveries-updated");
  await page.getByTestId("topic-display").fill("larder deliveries");
  await expect(page.getByTestId("create-sentence")).toHaveText("This creates a standard topic named deliveries-updated in us-east-1, with no subscriptions yet.");
  await expect(page.getByTestId("field-name")).toContainText("alerts and orders-created already exist in us-east-1.");
  await expect(page.getByTestId("section-details")).toContainText("Stored, not applied locally. doze-aws sends no email.");
  // Encryption: a key chosen from KMS's aliases, and what is known of it.
  await page.getByTestId("sse-on").click();
  await page.getByRole("combobox", { name: "KMS key" }).click();
  await page.getByRole("option", { name: "alias/larder-app" }).click();
  await expect(page.getByTestId("field-kms")).toContainText("alias/larder-app is symmetric and enabled");
  await expect(page.getByTestId("section-encryption")).toContainText("Stored, not applied locally.");
  // Delivery retry policy says it is applied to HTTP/S; delivery logging says it is only stored; Firehose and mobile push are off with their reasons.
  await expect(page.getByTestId("section-retry")).toContainText("Applied to HTTP and HTTPS subscriptions.");
  await expect(page.getByTestId("section-retry")).toContainText("Stored, not applied: maxReceivesPerSecond (there is no throttling) and headerContentType");
  // The boards fold only the FIFO form: a standard one has no Collapse buttons, and its Remove tag button is whole inside the card.
  await expect(page.getByRole("button", { name: "Collapse" })).toHaveCount(0);
  await expect(page.getByTestId("logging-Firehose")).toContainText("Not available in doze-aws");
  await expect(page.getByTestId("logging-Application")).toContainText("Disabled");
  await expect(page.getByTestId("section-logging")).toContainText("writes no delivery logs");
  await expect(page.getByTestId("section-tracing")).toContainText("doze-aws has no X-Ray, so the setting is only recorded.");
  // Tags.
  await page.getByTestId("section-tags").getByRole("button", { name: "Add tag" }).click();
  const tags = page.getByTestId("form-tags");
  await tags.getByRole("textbox").nth(0).fill("stack");
  await tags.getByRole("textbox").nth(1).fill("larder");
  await page.getByTestId("section-tags").getByRole("button", { name: "Add tag" }).click();
  await tags.getByRole("textbox").nth(2).fill("app");
  await tags.getByRole("textbox").nth(3).fill("fulfilment");
  const call = page.getByTestId("call-panel-code").first();
  await expect(call).toContainText("CreateTopic");
  await expect(call).toContainText("alias/larder-app");
  await expect(call).toContainText("app = fulfilment");
  await shot(page, "sns__topic-create");
  // It is made: the new topic's page, with its own banner. CreateTopic is idempotent, so the same call again answers the same ARN.
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("Topic deliveries-updated created.");
  await expect(page.getByTestId("field-Display name")).toHaveText("larder deliveries");
  expect(cli(["sns", "list-tags-for-resource", "--resource-arn", ARN("deliveries-updated")])).toContain("fulfilment");
  expect(cli(["sns", "get-topic-attributes", "--topic-arn", ARN("deliveries-updated")])).toContain("alias/larder-app");
  cli(["sns", "delete-topic", "--topic-arn", ARN("deliveries-updated")]);
});

test("sns__topic-create-fifo: FIFO options, and the other sections folded with what they hold", async ({ page }) => {
  await go(page, "/resources/sns/topics?create=yes", "Create topic");
  await page.getByTestId("type-fifo").click();
  await page.getByTestId("topic-name").fill(FIFO);
  await expect(page.getByTestId("fifo-note")).toContainText("A FIFO queue called orders-events.fifo already exists in SQS.");
  await expect(page.getByTestId("create-sentence")).toContainText("This creates a FIFO topic named orders-events.fifo. Publishers must send a message group ID and, because deduplication is by ID, a deduplication ID on every message.");
  await expect(page.getByTestId("section-fifo")).toContainText("FifoThroughputScope");
  await expect(page.getByTestId("section-size-summary")).toHaveText("256 KiB (default)");
  await expect(page.getByTestId("section-encryption-summary")).toHaveText("Off");
  await expect(page.getByTestId("section-access-summary")).toHaveText("Default: only the topic owner can publish or subscribe");
  await expect(page.getByTestId("section-logging-summary")).toHaveText("Off");
  await expect(page.getByTestId("section-tags-summary")).toHaveText("No tags");
  await expect(page.getByTestId("call-panel-code").first()).toContainText("FifoTopic = true");
  await shot(page, "sns__topic-create-fifo");
});

test("sns__topic-create-refused: a FIFO name without .fifo is refused, and the form says why", async ({ page, refusals }) => {
  // SNS refuses the name, and the form says why.
  refusals.allow(/400 POST .*\/sns\/topics\/op\/CreateTopic$/);
  await go(page, "/resources/sns/topics?create=yes", "Create topic");
  await page.getByTestId("type-fifo").click();
  await page.getByTestId("topic-name").fill("orders-events");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("create-refused")).toContainText("CreateTopic returned InvalidParameter:");
  await expect(page.getByTestId("create-refused")).not.toContainText("Exception");
  await expect(page.getByTestId("create-refused")).toContainText("For a FIFO topic, the name must end with the .fifo suffix.");
  await expect(page.getByTestId("field-name")).toContainText("For a FIFO topic, the name must end with the .fifo suffix.");
  await expect(page.getByTestId("call-panel-came-back")).toContainText("InvalidParameter");
  await expect(page.getByText("Nothing was created. Fix the name and try again.")).toBeVisible();
  await shot(page, "sns__topic-create-refused");
  expect(aws(server.endpoint, ["sns", "get-topic-attributes", "--topic-arn", ARN("orders-events")]).code).not.toBe(0);
});

test("an invalid name is marked on its field and brought into view, and a missing topic is not a down server", async ({ page, refusals }) => {
  // A page of what is not there asks the service, which refuses.
  refusals.allow(/404 GET .*\/sns\/topics\/no-such-topic$/);
  await go(page, "/resources/sns/topics?create=yes", "Create topic");
  await page.getByTestId("topic-name").fill("bad name!");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("field-name")).toContainText("1 to 256 characters: letters, numbers, underscores and hyphens.");
  await expect(page.getByTestId("field-name")).toHaveAttribute("data-refused", "");
  await expect(page.getByTestId("topic-name")).toBeFocused();
  await expect(page.getByTestId("topic-name")).toBeInViewport();
  await go(page, "/resources/sns/topics?create=yes", "Create topic");
  await page.getByTestId("topic-name").fill("n".repeat(257));
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("topic-name")).toBeFocused();
  await expect(page.getByTestId("disabled-why")).toContainText("1 to 256 characters");
  // A topic that is not there: the service's own words and code, and no advice to check that doze-aws runs (it answered).
  await page.goto(`${server.console}${topicPath("no-such-topic")}`);
  const err = page.getByTestId("detail-error");
  await expect(err).toContainText("Topic does not exist");
  await expect(err).toContainText("NotFound · HTTP 404");
  await expect(err).not.toContainText("Exception");
  await expect(err).not.toContainText("Check that doze-aws is still running");
  await expect(err).toContainText("There is no topic of that name in this account and Region.");
});

test("sns__topic-created and sns__topic-subscriptions-none: the FIFO topic is made and says what to do next", async ({ page }) => {
  await go(page, "/resources/sns/topics?create=yes", "Create topic");
  await page.getByTestId("type-fifo").click();
  await page.getByTestId("topic-name").fill(FIFO);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText(`Topic ${FIFO} created.`);
  await expect(page.getByTestId("detail-sentence")).toContainText("orders-events.fifo has no subscriptions yet, so a publish would reach nobody. Subscribe the FIFO queue orders-events.fifo from SQS to start delivering.");
  await expect(page.getByTestId("detail-badge")).toHaveText("FIFO");
  await expect(page.getByTestId("tab-panel")).toContainText("To subscribe to a FIFO topic, choose the Amazon SQS protocol.");
  await expect(page.getByTestId("tab-archive")).toBeVisible();
  await expect(page.getByTestId("tab-retry")).toHaveCount(0);
  await shot(page, "sns__topic-created");
  // The same page once the banner is dismissed.
  await page.getByTestId("created-banner-dismiss").click();
  await expect(page.getByTestId("created-banner")).toHaveCount(0);
  await expect(page.getByTestId("subscriptions-empty")).toContainText("No subscriptions");
  await shot(page, "sns__topic-subscriptions-none");
});

test("sns__subscription-create and sns__subscription-detail: a FIFO queue subscribed with a filter policy, confirmed at once", async ({ page }) => {
  await go(page, topicPath(FIFO), FIFO);
  await page.getByTestId("sub-create").click();
  await expect(page.locator("h1")).toHaveText("Create subscription");
  await expect(page.getByTestId("field-topic")).toContainText(ARN(FIFO));
  // The protocol list: five of nine are off, each with its reason.
  const list = page.getByTestId("protocol-list");
  for (const off of ["email", "email-json", "firehose", "application", "sms"]) await expect(list.getByTestId(`protocol-${off}`)).toHaveAttribute("data-disabled", "");
  await expect(list.getByTestId("protocol-email")).toContainText("Not available in doze-aws: it would need a mail server. Subscriptions are stored and never delivered.");
  await expect(list.getByTestId("protocol-sqs")).toContainText("Delivers locally; the choice for a FIFO topic.");
  if (browserKind === "chromium") await page.screenshot({ path: join(SHOTS, "sns__subscription-create.protocols.png") });
  await page.getByRole("combobox", { name: "Endpoint" }).click();
  await page.getByRole("option", { name: FIFO }).click();
  await expect(page.getByTestId("field-endpoint")).toContainText("FIFO topics use FIFO queues. orders-events.fifo is a FIFO queue.");
  await expect(page.getByTestId("create-sentence")).toHaveText("This subscribes the FIFO queue orders-events.fifo to the FIFO topic orders-events.fifo. The queue is in your account, so the subscription is confirmed at once.");
  await expect(page.getByTestId("section-redrive")).toContainText("Attribute: RedrivePolicy.");
  await page.getByTestId("filter-json").fill('{\n  "event": ["order_placed", "order_cancelled"]\n}');
  await expect(page.getByTestId("section-filter")).toContainText("A policy has at most 5 keys and 150 combinations of values");
  await expect(page.getByTestId("protocol-table")).toContainText("Five of the nine AWS protocols are disabled here, with the reason.");
  const call = page.getByTestId("call-panel-code").first();
  await expect(call).toContainText("Subscribe");
  await expect(call).toContainText("FilterPolicy");
  await shot(page, "sns__subscription-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("Subscription created.");
  await expect(page.locator("h1")).toContainText("…");
  await expect(page.getByTestId("detail-sentence")).toHaveText("Messages published to orders-events.fifo reach the queue orders-events.fifo only when their event attribute is order_placed or order_cancelled. Raw message delivery is off.");
  await expect(page.getByTestId("wired-filter")).toHaveText("filter: event in [order_placed, order_cancelled]");
  await expect(page.getByTestId("field-Status")).toHaveText("Confirmed");
  await expect(page.getByTestId("filter-section")).toContainText("Scope: attribute-based filtering (FilterPolicyScope = MessageAttributes)");
  await expect(page.getByTestId("redrive-section")).toContainText("Not set. Messages that cannot be delivered are not held in a dead-letter queue.");
  await shot(page, "sns__subscription-detail");
  // The Subscribe that made it is among its calls (a Subscribe names the topic, so the page borrows it from the topic just after creating).
  await expect(page.getByTestId("recent-calls")).toContainText("Subscribe");
});

test("sns__subscription-create-http: a webhook must confirm, and the answer is pending", async ({ page }) => {
  await go(page, `/resources/sns/subscriptions?create=yes&from=${encodeURIComponent(ARN("alerts"))}`, "Create subscription");
  await page.getByTestId("protocol-http").click();
  await page.getByTestId("sub-endpoint").fill(hook.url);
  await expect(page.getByTestId("field-protocol")).toContainText("HTTP delivery is an HTTP POST of the JSON-encoded message. HTTPS does the same over https://. A URL on this machine works.");
  await expect(page.getByTestId("create-sentence")).toHaveText(`This subscribes the webhook ${hook.url} to alerts. SNS will send a confirmation message to it, and nothing is delivered until the endpoint confirms.`);
  await expect(page.getByTestId("section-details")).toContainText("The endpoint must confirm.");
  await expect(page.getByTestId("section-details")).toContainText("The token is valid for two days.");
  await expect(page.getByTestId("section-filter-summary")).toHaveText("No filter policy: every message published to the topic is delivered");
  await expect(page.getByTestId("call-panel-code").first()).toContainText("Protocol = http");
  await shot(page, "sns__subscription-create-http");
  await page.getByTestId("form-submit").click();
  // Pending: the subscription has an ARN already (ReturnSubscriptionArn), so its page opens, saying it waits.
  await expect(page.getByTestId("created-banner")).toContainText("Subscribe returned pending confirmation");
  await expect(page.getByTestId("field-Status")).toHaveText("Pending confirmation");
  await expect(page.getByTestId("detail-sentence")).toContainText("is waiting to be confirmed");
  await expect(page.getByTestId("verb-delete")).toBeDisabled();
  await expect.poll(() => hook.subscribeUrl(), { timeout: 15_000 }).toContain("Token=");
});

test("sns__subscription-create-lambda: a function in the account is confirmed at once", async ({ page }) => {
  await go(page, `/resources/sns/subscriptions?create=yes&from=${encodeURIComponent(ARN("orders-created"))}`, "Create subscription");
  await page.getByTestId("protocol-lambda").click();
  await page.getByRole("combobox", { name: "Endpoint" }).click();
  await page.getByRole("option", { name: "send-email" }).click();
  await expect(page.getByTestId("create-sentence")).toHaveText("This subscribes the function send-email to orders-created. The function is in your account, so the subscription is confirmed at once and every publish invokes it.");
  await expect(page.getByTestId("field-endpoint")).toContainText(/send-email \(\S+\) is already triggered by the alerts topic\./);
  await expect(page.getByTestId("field-protocol")).toContainText("The Lambda function is invoked with the payload of the published message.");
  await expect(page.getByTestId("call-panel-code").first()).toContainText("Protocol = lambda");
  await shot(page, "sns__subscription-create-lambda");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("It is confirmed at once");
  // The seeded world has three queues on orders-created: the function's subscription is taken away again.
  const mine = subs("orders-created").find((s) => s.Protocol === "lambda");
  expect(mine).toBeTruthy();
  cli(["sns", "unsubscribe", "--subscription-arn", mine!.SubscriptionArn]);
});

test("sns__topic-alerts: a function confirmed, a webhook and an address waiting, and what a publish reaches", async ({ page }) => {
  await go(page, topicPath("alerts"), "alerts");
  await expect(page.getByTestId("detail-sentence")).toHaveText(`alerts has 3 subscriptions: the Lambda function send-email is confirmed, the webhook ${hook.url} waits for its confirmation, and ops@larder.example is stored and stays pending because doze-aws sends no email. A publish reaches only the confirmed one.`);
  const table = page.getByRole("table", { name: "Subscriptions" });
  await expect(table.locator("tr[data-row]")).toHaveCount(3);
  await expect(table).toContainText("PendingConfirmation");
  await expect(table).toContainText("Stored: doze-aws sends no email");
  await expect(table.locator("tr[data-row]", { hasText: "ops@larder.example" })).toContainText("Pending confirmation");
  await expect(table.locator("tr[data-row]", { hasText: "send-email" })).toContainText("AWS Lambda");
  await expect(page.getByTestId("sub-confirm")).toBeDisabled();
  // The webhook's row chosen, as the board has it: Confirm subscription is on.
  await page.getByTestId(`choose-pending|${ARN("alerts")}|http|${hook.url}`).click();
  await expect(page.getByTestId("sub-confirm")).toBeEnabled();
  await shot(page, "sns__topic-alerts");
});

test("sns__subscription-confirm: the SubscribeURL of the confirmation message, pasted, confirms (the dialog first)", async ({ page, refusals }) => {
  // A wrong token is SNS's refusal, shown in the dialog.
  refusals.allow(/400 POST .*\/sns\/subscriptions\/op\/ConfirmSubscription$/);
  await go(page, topicPath("alerts"), "alerts");
  await page.getByTestId(`choose-pending|${ARN("alerts")}|http|${hook.url}`).click();
  await page.getByTestId("sub-confirm").click();
  const dialog = page.getByTestId("confirm-dialog");
  await expect(dialog).toContainText(`The webhook ${hook.url} was subscribed to alerts. SNS posted it a confirmation message that carries a SubscribeURL.`);
  await expect(dialog.getByTestId("confirm-submit")).toBeDisabled();
  await shot(page, "sns__subscription-confirm");
  // A link with no token is not one; a token that is not the endpoint's is the service's refusal.
  await dialog.getByTestId("confirm-link").fill("http://example.com/nothing");
  await expect(dialog.getByTestId("confirm-wrong")).toBeVisible();
  await dialog.getByTestId("confirm-link").fill(`${server.endpoint}/_aws/sns/confirm?Action=ConfirmSubscription&TopicArn=${encodeURIComponent(ARN("alerts"))}&Token=not-the-token`);
  await dialog.getByTestId("confirm-submit").click();
  await expect(dialog.getByTestId("confirm-error")).toContainText("ConfirmSubscription was refused");
  await page.keyboard.press("Escape");
});

test("sns__topics-list and sns__subscriptions-list: three topics, seven subscriptions, what waits, and the last calls", async ({ page }) => {
  // What the apps did: a publish to each topic.
  cli(["sns", "publish", "--topic-arn", ARN("orders-created"), "--message", "Order 10001 placed", "--message-attributes", JSON.stringify({ event: { DataType: "String", StringValue: "order_placed" } })]);
  cli(["sns", "publish", "--topic-arn", ARN("alerts"), "--message", "disk is full"]);
  await go(page, "/resources/sns/topics", "Topics");
  await expect(page.getByTestId("list-sentence")).toContainText(`3 topics and 7 subscriptions in us-east-1. 2 are pending, both on alerts: the webhook ${hook.url} and the stored email ops@larder.example, which stays pending. Last publish: alerts at`);
  const table = page.getByRole("table", { name: "Topics" });
  await expect(table.locator("tr[data-row]")).toHaveCount(3);
  await expect(table.locator("tr[data-row]", { hasText: FIFO })).toContainText("FIFO");
  await expect(table.locator("tr[data-row]", { hasText: "alerts" })).toContainText(ARN("alerts"));
  await expect(page.getByTestId("action-edit")).toBeDisabled();
  await page.getByTestId("choose-orders-created").click();
  await expect(page.getByTestId("action-publish")).toBeEnabled();
  await expect(page.getByTestId("publish-help")).toContainText("With orders-events.fifo selected instead, it opens the Publish message page of the FIFO topic.");
  await expect(page.getByTestId("page-line")).toHaveText("3 topics, one ListTopics page (a page holds up to 100)");
  await expect(page.getByTestId("last-calls")).toContainText("Publish");
  await expect(page.getByTestId("last-calls")).toContainText("Last calls in SNS");
  // The feed is the same as the pages of a topic: the app's calls by their caller's name, this console's own reads hidden until asked for.
  await expect(page.getByTestId("last-calls").getByTestId("call-caller").first()).toContainText("AWS CLI");
  await expect(page.getByTestId("last-calls").getByTestId("calls-console-hidden")).toContainText("made by this console");
  await shot(page, "sns__topics-list");

  await page.getByTestId("sns-tab-subscriptions").click();
  await expect(page.locator("h1")).toHaveText("Subscriptions");
  await expect(page.getByTestId("list-sentence")).toHaveText(`7 subscriptions, 5 confirmed and 2 pending, both on alerts: the webhook ${hook.url} is waiting for its confirmation, and ops@larder.example is stored and stays pending because doze-aws sends no email.`);
  const subsTable = page.getByRole("table", { name: "Subscriptions" });
  await expect(subsTable.locator("tr[data-row]")).toHaveCount(7);
  await expect(subsTable.locator("tr[data-row]", { hasText: "Pending confirmation" })).toHaveCount(2);
  await expect(subsTable.locator("tr[data-row]", { hasText: "invoice-queue" })).toContainText("arn:…:orders-created");
  await expect(page.getByTestId("page-line")).toHaveText("7 subscriptions, one ListSubscriptions page (a page holds up to 100)");
  // A subscription that waits cannot be deleted, and the page says why; a link on its row opens its page (it was a broken one).
  await page.getByTestId(`choose-pending|${ARN("alerts")}|email|ops@larder.example`).click();
  await expect(page.getByTestId("action-delete")).toBeDisabled();
  await expect(page.getByTestId("delete-help")).toContainText("You can't delete a subscription that's pending confirmation.");
  await expect(page.getByTestId("delete-help")).toContainText("alerts has two");
  await page.getByTestId(`choose-${subs(FIFO)[0]!.SubscriptionArn}`).click();
  await expect(page.getByTestId("action-delete")).toBeEnabled();
  await shot(page, "sns__subscriptions-list");
});

test("sns__topic-detail: the strip from publisher to queues, the Details card, the subscriptions and the calls", async ({ page }) => {
  await go(page, topicPath("orders-created"), "orders-created");
  await expect(page.getByTestId("detail-sentence")).toHaveText("orders-created sends every publish to its 3 confirmed subscriptions: analytics-queue, inventory-queue and invoice-queue. Raw message delivery is off on all three, so each queue receives the full JSON envelope.");
  const strip = page.getByTestId("wired-strip");
  await expect(strip).toContainText("Publisher");
  await expect(strip).toContainText("AKIADOZEE2E00000001");
  await expect(strip).toContainText("SQS queues");
  await expect(strip.getByTestId("wired-link")).toHaveCount(3);
  await expect(page.getByTestId("field-Name")).toHaveText("orders-created");
  await expect(page.getByTestId("field-Display name")).toHaveText("larder orders");
  await expect(page.getByTestId("field-Topic owner")).toHaveText(server.account);
  for (const tab of ["subscriptions", "access", "retry", "logging", "encryption", "tags"]) await expect(page.getByTestId(`tab-${tab}`)).toBeVisible();
  await expect(page.getByTestId("tab-archive")).toHaveCount(0);
  await expect(page.getByRole("table", { name: "Subscriptions" }).locator("tr[data-row]")).toHaveCount(3);
  await expect(page.getByTestId("recent-calls")).toContainText("Publish");
  // SNS made one delivery to each of the three queues, over the bus; the publish says so.
  await expect(page.getByTestId("recent-calls").getByTestId("answer-words").filter({ hasText: "delivered to 3" }).first()).toBeVisible();
  await shot(page, "sns__topic-detail");
});

test("sns__topic-access: the policy, who it lets in, and Edit", async ({ page }) => {
  await go(page, topicPath("orders-created", "?tab=access"), "orders-created");
  await expect(page.getByTestId("detail-sentence")).toHaveText(`orders-created is open to its owner only: one statement allows any principal in account ${server.account} to publish, subscribe and manage the topic.`);
  await expect(page.getByTestId("policy-json")).toContainText("__default_policy_ID");
  // The whole policy is shown: its Condition block is not below a scrolling box.
  await expect(page.getByTestId("policy-json")).toContainText("AWS:SourceOwner");
  expect(await page.getByTestId("policy-json").evaluate((e) => e.scrollHeight <= e.clientHeight + 1)).toBe(true);
  await expect(page.getByTestId("access-tab")).toContainText("AddPermission and RemovePermission");
  await expect(page.getByTestId("tab-edit")).toBeVisible();
  await shot(page, "sns__topic-access");
});

test("sns__topic-delivery-policy: each attribute, its value and its rule; what doze-aws applies; the AWS schedule", async ({ page }) => {
  await go(page, topicPath("orders-created", "?tab=retry"), "orders-created");
  await expect(page.getByTestId("detail-sentence")).toHaveText("orders-created sets no delivery policy of its own, and it has no HTTP/S subscriptions. Amazon SQS and AWS Lambda deliveries are one attempt.");
  const table = page.getByRole("table", { name: "Delivery retry policy" });
  await expect(table.locator("tr[data-row]")).toHaveCount(9);
  await expect(table.locator("tr[data-row]", { hasText: "minDelayTarget" })).toContainText("20 seconds");
  await expect(table.locator("tr[data-row]", { hasText: "maxReceivesPerSecond" })).toContainText("No throttling");
  await expect(page.getByTestId("retry-applied")).toHaveText("Applied locally");
  await expect(page.getByTestId("retry-stored")).toContainText("Stored, not applied");
  await expect(page.getByTestId("retry-stored")).toContainText("maxReceivesPerSecond and headerContentType are kept and read back");
  await expect(page.getByTestId("retry-tab")).toContainText("100,015 attempts");
  await shot(page, "sns__topic-delivery-policy");
});

test("sns__topic-logging: per protocol, the roles and the rate, and that nothing is written", async ({ page }) => {
  await go(page, topicPath("orders-created", "?tab=logging"), "orders-created");
  await expect(page.getByTestId("detail-sentence")).toContainText("records delivery status logging for Amazon SQS only, with 100% of successful deliveries sampled. Nothing is written to CloudWatch Logs locally");
  const table = page.getByRole("table", { name: "Delivery status logging" });
  await expect(table.locator("tr[data-row]", { hasText: "Amazon SQS" })).toContainText("Success: role/sns-delivery-logs");
  await expect(table.locator("tr[data-row]", { hasText: "Amazon SQS" })).toContainText("100%");
  await expect(table.locator("tr[data-row]", { hasText: "HTTP" })).toContainText("Not set");
  await expect(table.locator("tr[data-row]", { hasText: "Amazon Data Firehose" })).toContainText("Not available in doze-aws");
  await expect(page.getByTestId("logging-tab")).toContainText("Stored, not applied locally.");
  await shot(page, "sns__topic-logging");
});

test("sns__topic-encryption: the key, what it is, and that nothing is encrypted", async ({ page }) => {
  await go(page, topicPath("orders-created", "?tab=encryption"), "orders-created");
  await expect(page.getByTestId("detail-sentence")).toHaveText("orders-created records the KMS key alias/larder-app as its encryption key. Nothing is encrypted locally; the setting round-trips for templates and apps that read it.");
  await expect(page.getByTestId("field-Encryption status")).toHaveText("Enabled");
  await expect(page.getByTestId("field-Customer managed key")).toContainText("alias/larder-app");
  await expect(page.getByTestId("field-Customer managed key ARN")).toContainText("arn:aws:kms:us-east-1:");
  await expect(page.getByTestId("field-Description")).toHaveText("Larder application data key");
  await expect(page.getByTestId("encryption-tab")).toContainText("Stored, not applied locally.");
  await shot(page, "sns__topic-encryption");
});

test("sns__topic-tags and sns__topic-tags-none: the tags a topic carries, and a topic with none", async ({ page }) => {
  await go(page, topicPath("orders-created", "?tab=tags"), "orders-created");
  await expect(page.getByTestId("detail-sentence")).toContainText("orders-created carries 2 tags: app and stack. Tags change nothing about how it delivers");
  await expect(page.getByTestId("tags-count")).toHaveText("2 tags");
  await expect(page.getByRole("table", { name: "Tags" })).toContainText("larder");
  await expect(page.getByTestId("tags-note")).toContainText("A topic cannot take more than 50 tags (TagLimitExceeded).");
  await shot(page, "sns__topic-tags");
  await go(page, topicPath("alerts", "?tab=tags"), "alerts");
  await expect(page.getByTestId("empty-tab")).toContainText("No tags");
  await expect(page.getByTestId("empty-tab")).toContainText("alerts has no tags. Tags are key-value labels for organising topics and tracking their cost.");
  await shot(page, "sns__topic-tags-none");
});

test("sns__publish-message: one message, a custom payload for each protocol, and who gets what", async ({ page }) => {
  await go(page, topicPath("orders-created", "?view=publish"), "Publish message to topic");
  await expect(page.getByTestId("publish-arn")).toHaveValue(ARN("orders-created"));
  await expect(page.getByTestId("publish-sentence")).toHaveText("This sends one message to 3 confirmed subscriptions.");
  await page.getByTestId("publish-subject").fill("Order 10482 placed");
  await page.getByTestId("structure-custom").click();
  await page.getByTestId("publish-message").fill('{\n  "default": "Order 10482 placed: 3 lines, 41.80 USD",\n  "sqs": "{\\"order_id\\":\\"10482\\",\\"lines\\":3,\\"total\\":\\"41.80\\"}"\n}');
  await expect(page.getByTestId("publish-sentence")).toHaveText("This sends one message to 3 confirmed subscriptions. Each SQS queue is given the text of the sqs key; a protocol with no key of its own would get default.");
  await expect(page.getByTestId("publish-size")).toContainText("of the 262,144-byte limit");
  await expect(page.getByTestId("section-attributes")).toContainText("Message attributes are sent only when the message structure is String, not JSON.");
  await expect(page.getByTestId("who-gets-what")).toContainText("Raw delivery is off on all 3");
  await expect(page.getByTestId("who-gets-what")).toContainText("the sqs text");
  await expect(page.getByTestId("call-panel-code").first()).toContainText("MessageStructure = json");
  await shot(page, "sns__publish-message");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("published-banner")).toBeVisible();
  const got = JSON.parse(cli(["sqs", "receive-message", "--queue-url", `${server.endpoint}/${server.account}/invoice-queue`, "--max-number-of-messages", "10"])) as { Messages: { Body: string }[] };
  // The queue is given the text of the sqs key of the custom payload (after the one the earlier CLI publish left).
  expect(got.Messages.map((m) => (JSON.parse(m.Body) as { Message: string }).Message)).toContain('{"order_id":"10482","lines":3,"total":"41.80"}');
});

test("sns__publish-fifo, sns__publish-fifo-refused and sns__topic-published: a group and a deduplication ID, the missing one refused, the filter checked", async ({ page, refusals }) => {
  // A publish without a deduplication ID is SNS's refusal, shown on the form.
  refusals.allow(/400 POST .*\/sns\/topics\/orders-events\.fifo\/op\/Publish$/);
  await go(page, topicPath(FIFO, "?view=publish"), "Publish message to topic");
  await page.getByTestId("publish-subject").fill("Order 10482 placed");
  await page.getByTestId("publish-group").fill("order-10482");
  await page.getByTestId("publish-message").fill('{"event":"order_placed","order_id":"10482","total":"41.80"}');
  await page.getByTestId("attr-add").click();
  await page.getByTestId("attr-name").fill("event");
  await page.getByTestId("attr-value").fill("order_placed");
  await expect(page.getByTestId("field-dedup")).toContainText("This topic has content-based deduplication off, so the ID is required.");
  await expect(page.getByTestId("publish-sentence")).toHaveText("This sends one message in group order-10482. The topic has 1 confirmed subscription and the message carries event = order_placed, which its filter policy accepts.");
  await expect(page.getByTestId("filter-verdict")).toHaveText("Matches, delivered");
  await expect(page.getByTestId("filter-preview")).toContainText("event in [order_placed, order_cancelled]");
  // Refused first: no deduplication ID.
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("publish-refused")).toContainText("Nothing was published.");
  await expect(page.getByTestId("publish-refused")).toContainText("MessageDeduplicationId");
  await expect(page.getByTestId("field-dedup")).toContainText("Required: this topic does not have ContentBasedDeduplication set.");
  await expect(page.getByTestId("call-panel-came-back")).toContainText("InvalidParameter");
  await shot(page, "sns__publish-fifo-refused");
  await page.getByTestId("publish-dedup").fill("order-10482-placed");
  // Editing the form puts the refusal away: the banner and the "refused" label do not stay over a good call.
  await expect(page.getByTestId("publish-refused")).toHaveCount(0);
  await expect(page.getByTestId("call-panel-came-back")).toHaveCount(0);
  await expect(page.getByTestId("call-panel-code").first()).toContainText("MessageDeduplicationId");
  await expect(page.getByTestId("call-panel-code").first()).toContainText("order-10482-placed");
  await shot(page, "sns__publish-fifo");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("published-banner")).toContainText("Message published.");
  await expect(page.getByTestId("detail-sentence")).toHaveText("The message in group order-10482 went to 1 of 1 confirmed subscription: orders-events.fifo. It matched the filter policy (event in [order_placed, order_cancelled]).");
  await expect(page.getByTestId("published-message-id")).not.toBeEmpty();
  await expect(page.getByTestId("published-sequence")).toContainText(/^\d+$/);
  await shot(page, "sns__topic-published");
  // The publish just made is in view, as this console's own, though the console's reads stay hidden.
  await expect(page.getByTestId("recent-calls")).toContainText("Publish");
  await expect(page.getByTestId("recent-calls").getByTestId("call-caller").first()).toContainText("This console");
  await expect(page.getByTestId("calls-console-hidden")).toContainText("made by this console");
  const got = JSON.parse(cli(["sqs", "receive-message", "--queue-url", `${server.endpoint}/${server.account}/${FIFO}`, "--attribute-names", "All"])) as { Messages: { Body: string }[] };
  expect(JSON.parse(got.Messages[0]!.Body).Message).toContain("order_placed");
});

test("sns__topic-edit: only what changed is sent, one call each, and the tags in their own", async ({ page }) => {
  await go(page, topicPath("orders-created", "?view=edit"), "Edit orders-created");
  await expect(page.getByTestId("edit-type")).toHaveValue("Standard");
  await expect(page.getByTestId("edit-sentence")).toHaveText("Nothing has changed yet.");
  await expect(page.getByTestId("form-submit")).toBeDisabled();
  await page.getByTestId("topic-max-size").fill("512");
  await page.getByTestId("section-tags").getByRole("button", { name: "Add tag" }).click();
  const tags = page.getByTestId("form-tags");
  await tags.getByRole("textbox").nth(4).fill("owner");
  await tags.getByRole("textbox").nth(5).fill("orders");
  await expect(page.getByTestId("edit-sentence")).toContainText("You changed 2 settings: the maximum message size goes to 512 KiB and the tag owner is added. Nothing else will be sent.");
  await expect(page.getByTestId("section-size")).toContainText("This topic has 3 subscriptions, under the limit of 100, and all of them are Amazon SQS or AWS Lambda.");
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText("AttributeValue = 524288");
  await expect(call).toContainText("TagResource");
  await expect(call).toContainText("one call for each setting you changed");
  await expect(page.getByTestId("section-logging")).toContainText("Delivery status logging");
  await shot(page, "sns__topic-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("updated-banner")).toBeVisible();
  const attrs = JSON.parse(cli(["sns", "get-topic-attributes", "--topic-arn", ARN("orders-created")])) as { Attributes: Record<string, string> };
  expect(attrs.Attributes.MaximumMessageSize).toBe("524288");
  expect(cli(["sns", "list-tags-for-resource", "--resource-arn", ARN("orders-created")])).toContain("owner");
});

test("sns__subscription-edit: raw delivery and a dead-letter queue, one call each", async ({ page }) => {
  await go(page, "/resources/sns/subscriptions", "Subscriptions");
  const arn = subs(FIFO)[0]!.SubscriptionArn;
  await page.getByTestId(`choose-${arn}`).click();
  await page.getByTestId("action-edit").click();
  await expect(page.locator("h1")).toContainText(`Edit ${arn.split(":").pop()}`);
  await expect(page.getByTestId("edit-arn")).toHaveText(arn);
  await expect(page.getByTestId("edit-sentence")).toHaveText("Nothing has changed yet.");
  await page.getByTestId("sub-raw").click();
  await page.getByTestId("redrive-on").click();
  await page.getByRole("combobox", { name: "Dead-letter queue" }).click();
  await page.getByRole("option", { name: "orders-events-dlq.fifo" }).click();
  await expect(page.getByTestId("edit-sentence")).toHaveText("You turned raw message delivery on and set a dead-letter queue. The rest is unchanged. Saving sends 2 calls.");
  await expect(page.getByTestId("section-redrive")).toContainText("orders-events-dlq.fifo is a FIFO queue.");
  await expect(page.getByTestId("section-filter")).toContainText("FilterPolicyScope = MessageBody");
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText("SetSubscriptionAttributes");
  await expect(call).toContainText("RedrivePolicy");
  await shot(page, "sns__subscription-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("updated-banner")).toBeVisible();
  await expect(page.getByTestId("field-Raw message delivery")).toHaveText("Enabled");
  await expect(page.getByTestId("redrive-section")).toContainText("orders-events-dlq.fifo");
});

test("sns__subscription-confirm (the confirmation itself): the pasted SubscribeURL makes the webhook's subscription a confirmed one", async ({ page }) => {
  await go(page, topicPath("alerts"), "alerts");
  await page.getByTestId(`choose-pending|${ARN("alerts")}|http|${hook.url}`).click();
  await page.getByTestId("sub-confirm").click();
  await page.getByTestId("confirm-link").fill(hook.subscribeUrl()!);
  await page.getByTestId("confirm-submit").click();
  await expect(page.getByTestId("confirmed-banner")).toContainText("Subscription confirmed.");
  await expect(page.getByRole("table", { name: "Subscriptions" }).locator("tr[data-row]", { hasText: hook.url })).toContainText("Confirmed");
  expect(subs("alerts").filter((s) => s.SubscriptionArn !== "PendingConfirmation")).toHaveLength(2);
  // Now a publish reaches the webhook as a notification.
  cli(["sns", "publish", "--topic-arn", ARN("alerts"), "--message", "disk is full"]);
  await expect.poll(() => hook.posts().some((p) => p.type === "Notification"), { timeout: 15_000 }).toBe(true);
});

test("sns__subscription-delete: from the list, with what stops and what is not touched", async ({ page, refusals }) => {
  await go(page, "/resources/sns/subscriptions", "Subscriptions");
  const arn = subs(FIFO)[0]!.SubscriptionArn;
  await page.getByTestId(`choose-${arn}`).click();
  await page.getByTestId("action-delete").click();
  const dialog = page.getByTestId("delete-dialog");
  await expect(dialog).toContainText("The queue orders-events.fifo stops receiving what is published to the topic orders-events.fifo. The queue itself is not touched.");
  await expect(dialog).toContainText("After the subscription is deleted, it cannot be recovered.");
  await shot(page, "sns__subscription-delete");
  await dialog.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("deleted");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("list-sentence")).toContainText("6 subscriptions");
  expect(subs(FIFO)).toHaveLength(0);
});

test("sns__topic-archive: a FIFO topic's archive policy, set on its Edit page and read on its tab", async ({ page }) => {
  await go(page, topicPath(FIFO, "?view=edit"), `Edit ${FIFO}`);
  await expect(page.getByTestId("section-archive-summary")).toHaveText("No archive policy");
  await page.getByTestId("section-archive-toggle").click();
  await expect(page.getByTestId("section-archive")).toContainText("Message archiving keeps a single copy of all messages published to the topic");
  await page.getByTestId("topic-archive").fill("7");
  await expect(page.getByTestId("call-panel-code").first()).toContainText("MessageRetentionPeriod");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("updated-banner")).toBeVisible();
  await page.goto(`${server.console}${topicPath(FIFO, "?tab=archive")}`);
  await expect(page.getByTestId("detail-sentence")).toContainText("orders-events.fifo has an archive policy of 7 days, set on the Edit topic page after the topic was created. Nothing is archived or replayable locally");
  await expect(page.getByTestId("field-Status")).toHaveText("active");
  await expect(page.getByTestId("field-retention")).toHaveText("7 days");
  await expect(page.getByTestId("field-Retention policy")).toHaveText("Archive policy active, 7 days");
  await expect(page.getByTestId("archive-tab")).toContainText("Stored, not applied locally.");
  await shot(page, "sns__topic-archive");
});

test("sns__topic-delete and sns__topic-deleted: the subscriptions that go with the topic, named; publishers then get NotFound", async ({ page, refusals }) => {
  await go(page, "/resources/sns/topics", "Topics");
  await page.getByTestId("choose-orders-created").click();
  await page.getByTestId("action-delete").click();
  const dialog = page.getByTestId("delete-dialog");
  await expect(dialog).toContainText("You are deleting orders-created. Deleting a topic deletes all its subscriptions; orders-created has 3 confirmed ones: analytics-queue, inventory-queue and invoice-queue. The queues stay");
  await expect(dialog).toContainText("it receives an error that the topic doesn't exist");
  await expect(dialog.getByTestId("delete-dialog-submit")).toBeDisabled();
  await shot(page, "sns__topic-delete");
  await dialog.getByTestId("delete-dialog-input").fill("delete me");
  await dialog.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Topic orders-created deleted.");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("list-sentence")).toHaveText("2 topics left in us-east-1. A publish to orders-created now gets NotFound: the topic does not exist.");
  await expect(page.getByRole("table", { name: "Topics" }).locator("tr[data-row]")).toHaveCount(2);
  const refused = aws(server.endpoint, ["sns", "publish", "--topic-arn", ARN("orders-created"), "--message", "x"]);
  expect(refused.code).not.toBe(0);
  expect(refused.out).toContain("NotFound");
  await shot(page, "sns__topic-deleted");
});

test("sns__topics-none and sns__subscriptions-none: nothing left to list, and what to do", async ({ page }) => {
  for (const t of ["alerts", FIFO]) cli(["sns", "delete-topic", "--topic-arn", ARN(t)]);
  await go(page, "/resources/sns/topics", "Topics");
  await expect(page.getByTestId("list-empty")).toContainText("No topics");
  await expect(page.getByTestId("list-sentence")).toHaveText("No topics in us-east-1: ListTopics returned an empty list. A publish needs a topic, and a topic needs a subscription before anything is delivered.");
  await expect(page.getByTestId("list-empty")).toContainText("An account can create at most 100,000 standard topics and 1,000 FIFO topics. CreateTopic is idempotent");
  await shot(page, "sns__topics-none");
  await page.getByTestId("sns-tab-subscriptions").click();
  await expect(page.getByTestId("list-empty")).toContainText("No subscriptions");
  await expect(page.getByTestId("list-empty")).toContainText("Nothing is subscribed to a topic in this region.");
  await shot(page, "sns__subscriptions-none");
  expect(queueArn(server.account, "invoice-queue")).toContain("invoice-queue");
});
