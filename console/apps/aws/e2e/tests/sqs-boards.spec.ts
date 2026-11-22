// The 34 SQS boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder" world
// with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { start, type Running, aws } from "../server";
import { deadLettered, queueUrl, seedLarder } from "./sqs-seed";
import { expect, expectNoErrorAnswers, test } from "./fixtures";
import { expectCrumbsEnd } from "./crumbs-kit";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-sqs");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45240);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });

let server: Running;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};
const url = (q: string) => queueUrl(server.endpoint, server.account, q);

/** Photographs the page as the board is: light, then dark (the class the theme switch sets), in Chromium only. */
async function shot(page: Page, board: string) {
  if (browserKind !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
  // The whole page as one tall viewport: a full-page capture of a page with a fixed sidebar repeats the sidebar.
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

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  seedLarder(server.endpoint, server.account);
});
test.afterAll(async () => {
  await server.stop();
});

test("sqs__queues-list: every queue with what it holds, and a sentence", async ({ page }) => {
  await go(page, "/resources/sqs/queues", "Queues");
  await expect(page.getByTestId("list-sentence")).toContainText("7 queues hold 18 messages waiting and 1 in flight. pick-lists-dlq is the only dead-letter queue with a message in it.");
  await expect(page.getByRole("table", { name: "Queues" }).locator("tbody tr[data-row]")).toHaveCount(7);
  await expect(page.getByTestId("page-line")).toHaveText("1 to 7 of 7 queues");
  const fifo = page.getByRole("table", { name: "Queues" }).locator("tr[data-row]", { hasText: "orders-events.fifo" });
  await expect(fifo).toContainText("FIFO");
  await expect(fifo).toContainText("Enabled");
  await shot(page, "sqs__queues-list");
});

test("sqs__queues-empty: a prefix that matches nothing, with the way back", async ({ page }) => {
  await go(page, "/resources/sqs/queues", "Queues");
  await page.getByTestId("list-search").fill("billing");
  await page.getByTestId("list-search-go").click();
  await expect(page.getByTestId("list-no-match")).toContainText("ListQueues returned no queues");
  await expect(page.getByTestId("list-no-match")).toContainText("case-sensitive");
  await expect(page.getByTestId("search-show-all")).toHaveText("Show all 7 queues");
  await shot(page, "sqs__queues-empty");
  // A prefix, not a substring: "lists" matches no queue, "pick" matches two.
  await page.getByTestId("list-search").fill("lists");
  await page.getByTestId("list-search-go").click();
  await expect(page.getByTestId("list-no-match")).toBeVisible();
  await page.getByTestId("list-search").fill("pick");
  await page.getByTestId("list-search-go").click();
  await expect(page.getByRole("table", { name: "Queues" }).locator("tbody tr[data-row]")).toHaveCount(2);
  await page.getByTestId("search-clear").click();
  await expect(page.getByRole("table", { name: "Queues" }).locator("tbody tr[data-row]")).toHaveCount(7);
});

test("sqs__queue-create and sqs__queue-create-refused: the form, its call, and the name that is taken", async ({ page, refusals }) => {
  // The name that is taken is SQS's refusal, shown on the form.
  refusals.allow(/400 POST .*\/sqs\/queues\/op\/CreateQueue$/);
  await go(page, "/resources/sqs/queues?create=yes", "Create queue");
  await page.getByTestId("queue-name").fill("pick-lists");
  await expect(page.getByText("pick-lists already exists in this Region.")).toBeVisible();
  await expect(page.getByText("pick-lists uses 30 seconds.")).toBeVisible();
  // Retention names its neighbour, the dead-letter queue; the dead-letter hint names queues that are one, and what the first keeps.
  await expect(page.getByText(/pick-lists keeps 4 days\./)).toContainText("pick-lists-dlq keeps 14 days.");
  await expect(page.getByTestId("section-dlq")).toContainText("pick-lists-dlq is one; pick-lists-dlq keeps messages for 14 days.");
  await expect(page.getByTestId("section-dlq")).not.toContainText("invoice-queue");
  await page.getByTestId("queue-visibility").fill("60");
  await expect(page.getByTestId("call-panel-code")).toContainText('"VisibilityTimeout": "60"');
  await expect(page.getByTestId("call-panel-code")).toContainText('"QueueName": "pick-lists"');
  await page.getByTestId("call-panel-tab-cli").click();
  await expect(page.getByTestId("call-panel-code")).toContainText("sqs create-queue --queue-name pick-lists");
  await page.getByTestId("call-panel-tab-boto3").click();
  await expect(page.getByTestId("call-panel-code")).toContainText("create_queue");
  await page.getByTestId("call-panel-tab-json").click();
  await expectCrumbsEnd(page, ["Queues","Create queue"]);
  await shot(page, "sqs__queue-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("create-refused")).toContainText("QueueNameExists");
  await expect(page.getByTestId("create-refused")).toContainText("different value for attribute VisibilityTimeout");
  await expect(page.getByTestId("field-name")).toContainText("A queue with this name already exists, with different attributes.");
  await expect(page.getByTestId("call-panel-came-back")).toContainText("QueueNameExists");
  await expect(page.getByTestId("what-it-has")).toContainText("VisibilityTimeout 30 seconds");
  await expect(page.getByTestId("open-existing")).toHaveText("Open pick-lists");
  await shot(page, "sqs__queue-create-refused");
  // The same attributes are not a refusal: SQS returns the queue's URL (what an app does on start-up).
  await page.getByTestId("queue-visibility").fill("30");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("queue-page")).toBeVisible();
});

test("sqs__queue-create-fifo and sqs__queue-created: a FIFO queue with its own settings and tags, and the page it lands on", async ({ page }) => {
  await go(page, "/resources/sqs/queues?create=yes", "Create queue");
  await page.getByTestId("type-fifo").click();
  await page.getByTestId("queue-name").fill("orders-events.fifo");
  await expect(page.getByText("orders-events.fifo already exists in this Region.")).toBeVisible();
  await page.getByTestId("queue-name").fill("stock-moves.fifo");
  await page.getByTestId("queue-dedup").click();
  await page.getByTestId("form-tags-add").click();
  await page.getByTestId("form-tags-key-0").fill("stack");
  await page.getByTestId("form-tags-value-0").fill("larder");
  await page.getByTestId("form-tags-add").click();
  await page.getByTestId("form-tags-key-1").fill("app");
  await page.getByTestId("form-tags-value-1").fill("fulfilment");
  await expect(page.getByTestId("call-panel-code")).toContainText('"FifoQueue": "true"');
  await expect(page.getByTestId("call-panel-code")).toContainText('"ContentBasedDeduplication": "true"');
  await expect(page.getByTestId("call-panel-code")).toContainText('"stack": "larder"');
  await expect(page.getByText("FIFO throughput limit is validated and returned; nothing is throttled.")).toBeVisible();
  // No FIFO queue is a dead-letter queue yet: the note says there is no good target.
  await expect(page.getByTestId("section-dlq")).toContainText("none is a dead-letter queue yet, so there is no good target. Create one first; SQS does not create it for you.");
  await shot(page, "sqs__queue-create-fifo");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("Queue stock-moves.fifo created. CreateQueue returned its URL");
  await expect(page.getByTestId("detail-sentence")).toContainText("stock-moves.fifo is empty. Every message you send needs a message group ID");
  await expect(page.getByTestId("detail-badge")).toHaveText("FIFO");
  await expect(page.getByTestId("field-Content-based deduplication")).toHaveText("Enabled");
  await expect(page.getByTestId("dlq-none")).toContainText("Edit the queue to add a FIFO dead-letter queue");
  await shot(page, "sqs__queue-created");
});

test("sqs__queue-detail: the sentence, what the queue is wired to, its verbs, details and the dead-letter queue", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists", "pick-lists");
  await expect(page.getByTestId("detail-sentence")).toContainText("pick-lists has 2 messages waiting and 1 in flight. A message received more than 5 times moves to pick-lists-dlq, which holds 1 now.");
  await expect(page.getByTestId("wired-strip")).toContainText("payment-settled rule, bus larder");
  await expect(page.getByTestId("wired-strip")).toContainText("pick-lists-dlq");
  await expect(page.getByTestId("wired-strip")).toContainText("alias/larder-app key");
  await expect(page.getByTestId("wired-self")).toHaveText("pick-lists");
  for (const verb of ["edit", "delete", "purge", "messages"]) await expect(page.getByTestId(`verb-${verb}`)).toBeVisible();
  // Start DLQ redrive is there, greyed: pick-lists is no one's dead-letter queue.
  await expect(page.getByTestId("verb-redrive")).toBeDisabled();
  await expect(page.getByTestId("verb-reasons")).toHaveCount(0);
  // The whole name is bold in the sentence, never half of it (pick-lists-dlq is another name).
  await expect(page.getByTestId("detail-sentence").locator("span.font-mono")).toHaveText(["pick-lists", "pick-lists-dlq"]);
  await expect(page.getByTestId("field-ARN")).toContainText(`arn:aws:sqs:us-east-1:${server.account}:pick-lists`);
  await expect(page.getByTestId("field-Approx. messages")).toHaveText("2 available, 1 in flight, 0 delayed");
  await expect(page.getByTestId("dlq-words")).toContainText("which holds 1 message now and keeps messages for 14 days");
  await expect(page.getByTestId("allow-reading")).toContainText("Not set (all source queues may use pick-lists as a dead-letter queue)");
  await expect(page.getByTestId("recent-calls")).toBeVisible();
  // A send says how big its body was, a receive how many messages came back.
  await expect(page.getByTestId("recent-calls").getByTestId("answer-words").filter({ hasText: /^· (sent \d+ B|no messages|\d+ messages?)$/ }).first()).toBeVisible();
  await expectCrumbsEnd(page, ["Queues","pick-lists"]);
  await shot(page, "sqs__queue-detail");
});

test("sqs__queue-access-policy and sqs__queue-encryption: what the policy lets in, and what is stored", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists?tab=policy", "pick-lists");
  await expect(page.getByTestId("policy-json")).toContainText('"Sid": "payment-settled"');
  const rows = page.getByTestId("who-row");
  await expect(rows.nth(0)).toContainText(`Queue owner`);
  await expect(rows.nth(1)).toContainText("EventBridge");
  await expect(rows.nth(1)).toContainText("SendMessage, only from the rule payment-settled");
  await shot(page, "sqs__queue-access-policy");
  await page.goto(`${server.console}/resources/sqs/queues/pick-lists?tab=encryption`);
  await expect(page.getByTestId("sse-state")).toContainText("Enabled");
  await expect(page.getByTestId("sse-state")).toContainText("AWS Key Management Service key (SSE-KMS)");
  await expect(page.getByTestId("encryption-tab")).toContainText("alias/larder-app");
  await expect(page.getByTestId("encryption-tab")).toContainText("Customer managed, symmetric, enabled");
  await expect(page.getByTestId("encryption-tab")).toContainText("5 minutes");
  await expect(page.getByTestId("encryption-tab")).toContainText("Stored, not applied locally.");
  await expect(page.getByRole("complementary", { name: "Stored, not applied" })).toBeVisible();
  await shot(page, "sqs__queue-encryption");
});

test("sqs__queue-lambda-triggers and sqs__queue-lambda-configure: no trigger, then one made with the function's timeout checked", async ({ page, refusals }) => {
  // A trigger the visibility timeout refuses is Lambda's refusal, shown on the form.
  refusals.allow(/400 POST .*\/sqs\/queues\/pick-lists\/op\/CreateEventSourceMapping$/);
  await go(page, "/resources/sqs/queues/pick-lists?tab=lambda", "pick-lists");
  await expect(page.getByTestId("lambda-empty")).toContainText("No Lambda function polls pick-lists.");
  await expect(page.getByTestId("lambda-triggers").getByRole("heading", { name: "Lambda triggers (0)" })).toBeVisible();
  await expect(page.getByTestId("lambda-configure-open")).toHaveCount(2);
  await shot(page, "sqs__queue-lambda-triggers");
  await page.getByTestId("lambda-configure-open").first().click();
  await expect(page.locator("h1")).toHaveText("Configure Lambda function trigger");
  // The first function is chosen already, with its ARN under the picker; its timeout is checked at once.
  await expect(page.getByTestId("function-arn")).toContainText("function:export-report");
  await expect(page.getByTestId("visibility-check")).toContainText("export-report has a timeout of 2 minutes, longer than this queue's visibility timeout.");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("trigger-refused")).toContainText("visibility");
  await page.getByRole("combobox", { name: "Choose a function" }).click();
  await page.getByRole("option", { name: "send-email" }).click();
  await expect(page.getByTestId("call-panel-code")).toContainText('"FunctionName": "send-email"');
  await expect(page.getByTestId("call-panel-code")).toContainText(`arn:aws:sqs:us-east-1:${server.account}:pick-lists`);
  await shot(page, "sqs__queue-lambda-configure");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("trigger-banner")).toContainText("Lambda trigger configured.");
  await expect(page.getByTestId("lambda-triggers")).toContainText("send-email");
  const mappings = cli(["lambda", "list-event-source-mappings", "--event-source-arn", `arn:aws:sqs:us-east-1:${server.account}:pick-lists`]);
  expect(mappings).toContain("send-email");
  // The mapping polls the queue and would take the messages the next boards send and receive: it is taken away again.
  for (const m of (JSON.parse(mappings) as { EventSourceMappings: { UUID: string }[] }).EventSourceMappings) cli(["lambda", "delete-event-source-mapping", "--uuid", m.UUID]);
});

test("sqs__queue-sns-subscriptions and sqs__queue-sns-subscribe: no subscription, then one made and the policy that lets the topic send", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists?tab=sns", "pick-lists");
  await expect(page.getByTestId("sns-empty")).toContainText("pick-lists is not subscribed to any Amazon SNS topic");
  await expect(page.getByTestId("sns-empty")).toContainText("The topic orders-created delivers to analytics-queue, inventory-queue and invoice-queue today.");
  await expect(page.getByTestId("sns-subscriptions").getByRole("heading", { name: "Subscriptions (0)" })).toBeVisible();
  await shot(page, "sqs__queue-sns-subscriptions");
  await page.getByTestId("sns-subscribe-open").first().click();
  await expect(page.locator("h1")).toHaveText("Subscribe to Amazon SNS topic");
  // The topic with the most deliveries is chosen already, with its ARN under the picker.
  await expect(page.getByTestId("topic-arn")).toContainText(":orders-created");
  await expect(page.getByTestId("field-topic")).toContainText("orders-created already has 3 subscriptions");
  await expect(page.getByTestId("call-panel-code")).toContainText('"Protocol": "sqs"');
  await expect(page.getByTestId("subscribe-form")).toContainText("pick-lists has 1 statement today and would have 2.");
  await shot(page, "sqs__queue-sns-subscribe");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("subscribed-banner")).toBeVisible();
  await expect(page.getByTestId("sns-subscriptions")).toContainText("orders-created");
  expect(cli(["sqs", "get-queue-attributes", "--queue-url", url("pick-lists"), "--attribute-names", "Policy"])).toContain("sns.amazonaws.com");
});

test("sqs__queue-tagging and sqs__queue-tagging-none: the tags, and a queue with none", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists?tab=tags", "pick-lists");
  await expect(page.getByTestId("tags-count")).toHaveText("3 cost allocation tags");
  await expect(page.getByTestId("tags-note")).toContainText("Tags are case-sensitive and SQS reads them as plain strings.");
  const t = page.getByRole("table", { name: "Tags" });
  await expect(t).toContainText("fulfilment");
  await shot(page, "sqs__queue-tagging");
  await go(page, "/resources/sqs/queues/email-outbox?tab=tags", "email-outbox");
  await expect(page.getByTestId("empty-tab")).toContainText("No tags");
  await expect(page.getByTestId("empty-tab")).toContainText("email-outbox has no tags. A tag is a key and a value you attach to a queue to organise it or track its cost.");
  await shot(page, "sqs__queue-tagging-none");
  await page.getByTestId("tags-edit-queue").click();
  await expect(page.locator("h1")).toHaveText("Edit email-outbox");
});

test("sqs__queue-edit: the settings in their own page, only what changed sent, tags through TagQueue", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists?view=edit", "Edit pick-lists");
  await expect(page.getByTestId("disabled-why")).toHaveText("Nothing has changed.");
  await expect(page.getByTestId("policy-json").first()).toBeVisible();
  await page.getByTestId("queue-visibility").fill("60");
  await page.getByTestId("form-tags-add").click();
  await page.getByTestId("form-tags-key-3").fill("team");
  await page.getByTestId("form-tags-value-3").fill("floor");
  await expect(page.getByTestId("call-panel-code")).toContainText('"VisibilityTimeout": "60"');
  await expect(page.getByTestId("call-panel-code")).not.toContainText("MessageRetentionPeriod");
  await expectCrumbsEnd(page, ["pick-lists","Edit"]);
  await shot(page, "sqs__queue-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("updated-banner")).toContainText("Queue pick-lists updated.");
  await expect(page.getByTestId("field-Visibility timeout")).toHaveText("1 minute");
  const attrs = cli(["sqs", "get-queue-attributes", "--queue-url", url("pick-lists"), "--attribute-names", "VisibilityTimeout"]);
  expect(attrs).toContain('"60"');
  expect(cli(["sqs", "list-queue-tags", "--queue-url", url("pick-lists")])).toContain("floor");
  cli(["sqs", "set-queue-attributes", "--queue-url", url("pick-lists"), "--attributes", "VisibilityTimeout=30"]);
  cli(["sqs", "untag-queue", "--queue-url", url("pick-lists"), "--tag-keys", "team"]);
});

test("sqs__queue-messages, -message-sent, -messages-polled, message-detail, message-delete, message-deleted: send, poll, open and delete", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists?view=messages", "Send and receive messages");
  await expect(page.getByTestId("messages-page")).toContainText("Polling counts as receiving.");
  await shot(page, "sqs__queue-messages");
  await page.getByTestId("send-body").fill('{"pick_list":"PL-4472","order":"ord-20419","lines":7}');
  await page.getByTestId("send-delay").fill("1");
  await page.getByTestId("send-delay-unit").click();
  await page.getByRole("option", { name: "minutes" }).click();
  await expect(page.getByTestId("send-submit")).toBeEnabled();
  await page.getByTestId("send-delay").fill("");
  await page.getByTestId("send-add-attr").click();
  await page.getByLabel("Attribute name").fill("order_channel");
  await page.getByLabel("Attribute value").fill("web");
  await page.getByTestId("send-submit").click();
  await expect(page.getByTestId("sent-banner")).toContainText("Message sent. SendMessage returned MessageId");
  await expect(page.getByTestId("send-result")).toContainText("MD5OfMessageBody");
  await expect(page.getByTestId("send-result")).toContainText("MD5OfMessageAttributes");
  // The result is in the form's place, and Poll is the main button now.
  await expect(page.getByTestId("send-body")).toHaveCount(0);
  await expect(page.getByTestId("poll")).toHaveClass(/bg-primary/);
  await shot(page, "sqs__queue-message-sent");
  // Two more are waiting, so the poll returns several and a delete leaves some of them, as the boards show.
  for (const n of [1, 2]) cli(["sqs", "send-message", "--queue-url", url("pick-lists"), "--message-body", `pick list ${n}`]);
  await page.getByTestId("poll").click();
  const polled = page.getByTestId("polled");
  await expect(polled).toContainText("Messages (");
  // As the boards show a poll: the Receive card is its one line, and the Send form is not on the page (a small button brings it back).
  await expect(page.getByTestId("receive-card")).toHaveAttribute("data-compact", "");
  await expect(page.getByTestId("receive-card")).not.toContainText("Polling counts as receiving");
  await expect(page.getByTestId("send-body")).toHaveCount(0);
  await expect(page.getByTestId("send-folded")).toHaveCount(0);
  await expect(page.getByRole("table", { name: "Messages" }).locator("tbody tr[data-row]")).not.toHaveCount(0);
  await expect(page.getByRole("table", { name: "Messages" })).toContainText("bytes");
  const before = await page.getByRole("table", { name: "Messages" }).locator("tbody tr[data-row]").count();
  // Choose one, as the board does: the bar says so and Delete (at its right) is live.
  await expect(page.getByTestId("delete-picked")).toBeDisabled();
  await page.getByRole("table", { name: "Messages" }).locator("tbody tr[data-row]").last().getByRole("checkbox").click();
  await expect(page.getByTestId("selected-count")).toHaveText("1 selected");
  await shot(page, "sqs__queue-messages-polled");
  // Open one: its body, its attributes and the ReceiveMessage answer with the receipt handle.
  await page.getByTestId("message-open").last().click();
  await expect(page.getByTestId("message-sheet")).toContainText("Message details");
  await expect(page.getByTestId("message-answer")).toContainText("ReceiptHandle");
  await shot(page, "sqs__message-detail");
  await page.getByTestId("message-close").click();
  await page.getByTestId("delete-picked").click();
  await expect(page.getByTestId("delete-message-dialog")).toContainText("Delete 1 message from pick-lists?");
  await expect(page.getByTestId("delete-message-dialog")).toContainText("receipt handle");
  await shot(page, "sqs__message-delete");
  await page.getByTestId("delete-message-confirm").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Deleted 1 message from pick-lists. It is gone for good");
  await expect(page.getByTestId("deleted-note")).toContainText("Deleting needs the receipt handle");
  await expect(page.getByText(/messages are left from the last poll\./)).toBeVisible();
  await expect(page.getByRole("table", { name: "Messages" }).locator("tbody tr[data-row]")).toHaveCount(before - 1);
  await shot(page, "sqs__message-deleted");
  await page.getByTestId("send-show").click();
  await expect(page.getByTestId("send-body")).toBeVisible();
});

test("sqs__queue-messages-fifo: a FIFO queue needs a group, takes no per-message delay and starts empty", async ({ page }) => {
  await go(page, "/resources/sqs/queues/stock-moves.fifo?view=messages", "Send and receive messages");
  await expect(page.getByTestId("messages-page")).toContainText("FIFO queues don't support timers on individual messages");
  await expect(page.getByTestId("send-delay")).toHaveCount(0);
  await expect(page.getByTestId("no-messages")).toContainText("Send a message above, then poll.");
  await expect(page.getByTestId("messages-page")).toContainText("Not needed here: content-based deduplication is on");
  await shot(page, "sqs__queue-messages-fifo");
  await page.getByTestId("send-body").fill('{"sku":"FV-TOM-V","from":"bay-3","to":"aisle-B3","qty":24}');
  await expect(page.getByTestId("send-submit")).toBeDisabled();
  await page.getByTestId("send-group").fill("bay-3");
  await page.getByTestId("send-submit").click();
  await expect(page.getByTestId("sent-banner")).toBeVisible();
  await page.getByTestId("poll").click();
  await expect(page.getByTestId("polled")).toContainText("Messages (1)");
});

test("sqs__dlq-detail and sqs__dlq-redrive-failed: a message sent straight to the dead-letter queue has no source to return to", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists-dlq", "pick-lists-dlq");
  await expect(page.getByTestId("detail-badge").nth(1)).toHaveText("Dead-letter queue");
  await expect(page.getByTestId("detail-sentence")).toContainText("pick-lists-dlq holds 1 message. It is the dead-letter queue of pick-lists, which sends a message here once its receive count exceeds 5.");
  await expect(page.getByTestId("redrive-status")).toContainText("No tasks");
  await expect(page.getByTestId("redrive-status")).toContainText("pick-lists");
  await expect(page.getByTestId("verb-redrive")).toBeVisible();
  await shot(page, "sqs__dlq-detail");
  await page.getByTestId("verb-redrive").click();
  await expect(page.locator("h1")).toHaveText("Start DLQ redrive");
  await expect(page.getByTestId("call-panel-code")).toContainText(`arn:aws:sqs:us-east-1:${server.account}:pick-lists-dlq`);
  await expect(page.getByTestId("call-panel-code")).not.toContainText("DestinationArn");
  await shot(page, "sqs__dlq-redrive");
  await page.getByTestId("form-submit").click();
  await page.getByTestId("redrive-refresh").click();
  await expect(page.getByTestId("redrive-fields")).toHaveAttribute("data-status", "FAILED");
  // The start's banner does not stay over a task that failed.
  await expect(page.getByTestId("redrive-started-banner")).toHaveCount(0);
  await expect(page.getByTestId("redrive-note")).toContainText("Failed: CouldNotDetermineMessageSource.");
  await expect(page.getByTestId("redrive-Failure reason")).toHaveText("CouldNotDetermineMessageSource");
  await expect(page.getByTestId("detail-sentence")).toContainText("still holds 1 message. The last redrive failed because SQS has no source queue to return that message to.");
  await expect(page.getByTestId("redrive-note")).toContainText("custom destination");
  await shot(page, "sqs__dlq-redrive-failed");
});

test("sqs__dlq-redrive-running and -cancelled: a slow redrive is RUNNING, then cancelled with what it moved left alone", async ({ page }) => {
  deadLettered(server.endpoint, server.account, "slow-source", "slow-dlq", 8);
  await go(page, "/resources/sqs/queues/slow-dlq?view=redrive", "Start DLQ redrive");
  await page.getByTestId("rate-custom").click();
  await page.getByTestId("redrive-rate").fill("1");
  await expect(page.getByTestId("call-panel-code")).toContainText('"MaxNumberOfMessagesPerSecond": 1');
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("redrive-started-banner")).toBeVisible();
  await expect(page.getByTestId("redrive-fields")).toHaveAttribute("data-status", "RUNNING");
  await expect(page.getByTestId("redrive-Task handle")).toContainText("…");
  await expect(page.getByTestId("detail-sentence")).toContainText("slow-dlq is moving");
  // A running task is read again on its own (no Refresh), and the header's Start is greyed while it runs.
  await expect(page.getByTestId("verb-redrive")).toBeDisabled();
  const first = await page.getByTestId("redrive-Messages moved").innerText();
  await expect(page.getByTestId("redrive-Messages moved")).not.toHaveText(first, { timeout: 12_000 });
  await shot(page, "sqs__dlq-redrive-running");
  await page.getByTestId("redrive-cancel").click();
  await expect(page.getByTestId("redrive-fields")).toHaveAttribute("data-status", "CANCELLED");
  await expect(page.getByTestId("redrive-note")).toContainText("Status CANCELLED.");
  await expect(page.getByTestId("redrive-started-banner")).toHaveCount(0);
  await expect(page.getByTestId("redrive-Task handle")).toHaveText("Not returned (only while RUNNING)");
  await expect(page.getByTestId("detail-sentence")).toContainText("You cancelled the redrive");
  await shot(page, "sqs__dlq-redrive-cancelled");
});

test("sqs__dlq-redrive-completed: a message that was dead-lettered goes back to its source and the dead-letter queue is empty", async ({ page }) => {
  deadLettered(server.endpoint, server.account, "moved-source", "moved-dlq", 1);
  await go(page, "/resources/sqs/queues/moved-dlq?view=redrive", "Start DLQ redrive");
  await page.getByTestId("form-submit").click();
  await page.getByTestId("redrive-refresh").click();
  await expect(page.getByTestId("redrive-fields")).toHaveAttribute("data-status", "COMPLETED");
  await expect(page.getByTestId("redrive-completed-banner")).toContainText("Status COMPLETED. 1 message moved from moved-dlq to moved-source.");
  await expect(page.getByTestId("redrive-Messages moved")).toHaveText("1 of 1");
  await expect(page.getByTestId("detail-sentence")).toContainText("moved-dlq is empty. Its message went back to their source queues");
  expect(cli(["sqs", "get-queue-attributes", "--queue-url", url("moved-source"), "--attribute-names", "ApproximateNumberOfMessages"])).toContain('"1"');
  await shot(page, "sqs__dlq-redrive-completed");
});

test("sqs__queue-purge and sqs__queue-purged: the count in the confirm, then the queue emptied and what still feeds it", async ({ page, refusals }) => {
  // A second purge within a minute is SQS's refusal, shown in the dialog.
  refusals.allow(/400 POST .*\/sqs\/queues\/pick-lists\/op\/PurgeQueue$/);
  await go(page, "/resources/sqs/queues/pick-lists", "pick-lists");
  await page.getByTestId("verb-purge").click();
  const dialog = page.getByTestId("purge-dialog");
  await expect(dialog).toContainText("Purging pick-lists deletes all");
  await expect(dialog).toContainText("PurgeQueueInProgress");
  await expect(page.getByTestId("purge-dialog-submit")).toBeDisabled();
  await shot(page, "sqs__queue-purge");
  await page.getByTestId("purge-dialog-input").fill("purge");
  await page.getByTestId("purge-dialog-submit").click();
  await expect(page.getByTestId("purged-banner")).toContainText("pick-lists purged.");
  await expect(page.getByTestId("purged-feeding")).toContainText("The rule payment-settled still delivers to pick-lists");
  await expect(page.getByTestId("purged-banner")).toContainText(/(are|is) gone\./);
  await expect(page.getByTestId("detail-sentence")).toContainText("pick-lists was purged: 0 messages waiting and 0 in flight. The queue and its settings are unchanged.");
  await shot(page, "sqs__queue-purged");
  // A second purge inside 60 seconds is refused as AWS refuses it.
  await page.getByTestId("verb-purge").click();
  await page.getByTestId("purge-dialog-input").fill("purge");
  await page.getByTestId("purge-dialog-submit").click();
  await expect(page.getByTestId("purge-error")).toContainText("PurgeQueueInProgress");
  await page.keyboard.press("Escape");
});

test("sqs__queue-delete, sqs__queue-deleted and sqs__queues-none: the confirm names what goes and what feeds it, then the list, then no queue at all", async ({ page, refusals }) => {
  // The purge emptied it; the delete is photographed with messages in it, as the board is.
  for (const n of [1, 2]) cli(["sqs", "send-message", "--queue-url", url("pick-lists"), "--message-body", `pick list ${n}`]);
  await go(page, "/resources/sqs/queues/pick-lists", "pick-lists");
  await page.getByTestId("verb-delete").click();
  const dialog = page.getByTestId("delete-dialog");
  await expect(dialog).toContainText("Delete pick-lists and its 2 messages?");
  await expect(dialog).toContainText("QueueDeletedRecently");
  await expect(dialog).toContainText("The rule payment-settled targets this queue and will have nowhere to deliver.");
  await expect(dialog).toContainText("pick-lists-dlq keeps its 1 message.");
  await expect(page.getByTestId("delete-dialog-submit")).toBeDisabled();
  await shot(page, "sqs__queue-delete");
  await page.getByTestId("delete-dialog-input").fill("delete");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Queue pick-lists deleted.");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("deleted-banner")).toContainText("QueueDeletedRecently");
  await expect(page.getByTestId("list-sentence")).toContainText("queues remain, holding");
  await expect(page.getByTestId("list-sentence")).toContainText("pick-lists-dlq no longer has a source queue.");
  await expect(page.getByRole("table", { name: "Queues" }).getByRole("link", { name: "pick-lists", exact: true })).toHaveCount(0);
  await shot(page, "sqs__queue-deleted");
  const listed = JSON.parse(cli(["sqs", "list-queues"])) as { QueueUrls?: string[] };
  for (const u of listed.QueueUrls ?? []) cli(["sqs", "delete-queue", "--queue-url", u]);
  await page.getByTestId("refresh-now").click();
  await expect(page.getByTestId("list-empty")).toContainText("There are no queues in us-east-1. Create a queue to start sending and receiving messages.");
  await shot(page, "sqs__queues-none");
});
