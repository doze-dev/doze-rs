// The resource pages of their own against a real doze-aws: a queue's messages sent, peeked, opened, deleted and purged, its
// settings changed, a dead-letter queue redriven; a bucket's folders, an upload, an object's drawer, a download, a delete; a
// table's items created, scanned, queried, edited and deleted. Each call is the console's, and Live has it.
import { calls, CONSOLE, expect, open, test, unique } from "./fixtures";

const mine = async (page: Parameters<typeof calls>[0], action: string) => (await calls(page, CONSOLE)).calls.filter((c) => c.caller === "console" && c.action === action);

test("a queue's message is sent, peeked without hiding it, opened, deleted, and the queue purged", async ({ page, cli, browserName }) => {
  const queue = unique("pick", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", queue]).code).toBe(0);
  await open(page, `/resources/sqs/queues/${queue}?tab=messages`, queue);
  await expect(page.getByTestId("tab-messages")).toHaveAttribute("aria-current", "page");
  await page.getByTestId("send-body").fill('{"order_id": "ord-1"}');
  await page.getByTestId("send-add-attr").click();
  await page.getByLabel("Attribute name").fill("source");
  await page.getByLabel("Attribute value").fill("e2e");
  await page.getByTestId("send-submit").click();
  await expect(page.getByTestId("send-body")).toHaveValue("");
  for (const n of ["1", "2"]) {
    await page.getByTestId("peek").click();
    await expect(page.getByTestId("peeked").locator("tbody tr")).toHaveCount(1);
    await expect(page.getByTestId("peeked").locator("tbody tr").first().locator("td").last()).toHaveText(n);
  }
  await page.getByTestId("peeked").locator("tbody tr").first().click();
  const sheet = page.getByTestId("message-sheet");
  await expect(sheet.getByTestId("message-body")).toContainText('"order_id": "ord-1"');
  await expect(sheet).toContainText("source");
  await sheet.getByTestId("message-delete").click();
  await expect(sheet).toHaveCount(0);
  await expect(page.getByTestId("peeked").locator("tbody tr")).toHaveCount(0);
  expect(cli(["sqs", "send-message", "--queue-url", `${process.env.DOZE_AWS_E2E_ENDPOINT}/111122223333/${queue}`, "--message-body", "x"]).code).toBe(0);
  await page.getByTestId("purge-open").click();
  await page.getByTestId("purge-dialog-submit").click();
  await expect(page.getByTestId("purge-dialog")).toHaveCount(0);
  await page.getByTestId("peek").click();
  await expect(page.getByTestId("peeked")).toContainText("No message is waiting");
  for (const op of ["SendMessage", "ReceiveMessage", "DeleteMessage", "PurgeQueue"]) expect((await mine(page, op)).some((c) => c.outcome === "ok"), op).toBe(true);
});

test("a queue's settings are changed, and only what changed is sent", async ({ page, cli, browserName }) => {
  const queue = unique("settings", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", queue]).code).toBe(0);
  await open(page, `/resources/sqs/queues/${queue}?tab=settings`, queue);
  await page.getByTestId("set-VisibilityTimeout").fill("50000");
  await expect(page.getByTestId("settings-wrong")).toContainText("43,200");
  await page.getByTestId("set-VisibilityTimeout").fill("75");
  await page.getByTestId("settings-save").click();
  await expect(page.getByTestId("settings-save")).toBeDisabled();
  await page.getByTestId("tab-details").click();
  await expect(page.getByTestId("field-Visibility timeout")).toContainText("75");
});

test("a dead-letter queue's messages are moved to another queue, and the move is listed", async ({ page, cli, browserName }) => {
  const dlq = unique("dlq", browserName);
  const source = unique("src", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", dlq]).code).toBe(0);
  const policy = JSON.stringify({ deadLetterTargetArn: `arn:aws:sqs:us-east-1:111122223333:${dlq}`, maxReceiveCount: 3 });
  expect(cli(["sqs", "create-queue", "--queue-name", source, "--attributes", JSON.stringify({ RedrivePolicy: policy })]).code).toBe(0);
  expect(cli(["sqs", "send-message", "--queue-url", `${process.env.DOZE_AWS_E2E_ENDPOINT}/111122223333/${dlq}`, "--message-body", "lost"]).code).toBe(0);
  // A queue that is no one's dead-letter queue has no redrive.
  await open(page, `/resources/sqs/queues/${source}`, source);
  await expect(page.getByTestId("tab-redrive")).toHaveCount(0);
  await open(page, `/resources/sqs/queues/${dlq}?tab=redrive`, dlq);
  await expect(page.getByTestId("redrive-card")).toContainText(source);
  await page.getByText("To another queue").click();
  await page.getByTestId("redrive-queue").fill(source);
  await page.getByTestId("redrive-start").click();
  await expect(async () => {
    await page.getByTestId("moves-refresh").click();
    await expect(page.getByTestId("moves-card").locator("tbody tr").first()).toContainText("completed", { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await expect(page.getByTestId("moves-card").locator("tbody tr").first()).toContainText(source);
});

test("a message sent straight to a dead-letter queue has no queue to return to: the redrive fails and says why", async ({ page, cli, browserName }) => {
  const dlq = unique("dlqf", browserName);
  const source = unique("srcf", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", dlq]).code).toBe(0);
  const policy = JSON.stringify({ deadLetterTargetArn: `arn:aws:sqs:us-east-1:111122223333:${dlq}`, maxReceiveCount: 3 });
  expect(cli(["sqs", "create-queue", "--queue-name", source, "--attributes", JSON.stringify({ RedrivePolicy: policy })]).code).toBe(0);
  expect(cli(["sqs", "send-message", "--queue-url", `${process.env.DOZE_AWS_E2E_ENDPOINT}/111122223333/${dlq}`, "--message-body", "by hand"]).code).toBe(0);
  await open(page, `/resources/sqs/queues/${dlq}?tab=redrive`, dlq);
  await page.getByTestId("redrive-start").click();
  await expect(async () => {
    await page.getByTestId("moves-refresh").click();
    await expect(page.getByTestId("move-failed")).toContainText("CouldNotDetermineMessageSource", { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await expect(page.getByTestId("moves-card").locator("tbody tr").first()).toContainText("failed");
  await expect(page.getByTestId("moves-card").locator("tbody tr").first()).toContainText("0 of 1");
});

test("a bucket's folder is made and opened, a file uploaded into it, opened, downloaded and deleted", async ({ page, cli, browserName }) => {
  const bucket = unique("images", browserName);
  expect(cli(["s3api", "create-bucket", "--bucket", bucket]).code).toBe(0);
  await open(page, `/resources/s3/buckets/${bucket}?tab=objects`, bucket);
  await expect(page.getByTestId("objects-card")).toContainText("holds no object yet");
  await page.getByTestId("folder-open").click();
  await page.getByTestId("folder-name").fill("a/b");
  await expect(page.getByTestId("folder-submit")).toBeDisabled();
  await page.getByTestId("folder-name").fill("banners");
  await page.getByTestId("folder-submit").click();
  await expect(page.getByTestId("folder-dialog")).toHaveCount(0);
  await page.getByTestId("objects-card").locator("tbody tr", { hasText: "banners/" }).click();
  await expect(page).toHaveURL(/p=banners%2F|p=banners\//);
  await expect(page.getByTestId("object-crumbs")).toContainText("banners");
  await expect(page.getByTestId("objects-card")).toContainText("This folder is empty.");
  await page.getByTestId("upload-open").click();
  await page.getByTestId("upload-files").setInputFiles({ name: "hero.txt", mimeType: "text/plain", buffer: Buffer.from("hello, bucket") });
  await page.getByTestId("upload-submit").click();
  await expect(page.getByTestId("upload-dialog")).toHaveCount(0);
  const row = page.getByTestId("objects-card").locator("tbody tr", { hasText: "hero.txt" });
  await row.click();
  const sheet = page.getByTestId("object-sheet");
  await expect(sheet.getByTestId("object-head")).toContainText("text/plain");
  await expect(sheet.getByTestId("object-head")).toContainText("13 B");
  const [download] = await Promise.all([page.waitForEvent("download"), sheet.getByTestId("object-download").click()]);
  expect(download.suggestedFilename()).toBe("hero.txt");
  await sheet.getByTestId("object-delete").click();
  await page.getByTestId("object-delete-dialog-submit").click();
  await expect(sheet).toHaveCount(0);
  await expect(row).toHaveCount(0);
  const head = cli(["s3api", "list-objects-v2", "--bucket", bucket]);
  expect(head.out).not.toContain("hero.txt");
  expect((await mine(page, "PutObject")).length).toBeGreaterThanOrEqual(2);
});

test("a table's item is created in plain JSON, scanned, queried by its key, edited and deleted", async ({ page, cli, browserName }) => {
  const table = unique("orders", browserName);
  expect(cli(["dynamodb", "create-table", "--table-name", table, "--billing-mode", "PAY_PER_REQUEST", "--attribute-definitions", "AttributeName=pk,AttributeType=S", "AttributeName=sk,AttributeType=N", "--key-schema", "AttributeName=pk,KeyType=HASH", "AttributeName=sk,KeyType=RANGE"]).code).toBe(0);
  expect(cli(["dynamodb", "put-item", "--table-name", table, "--item", '{"pk":{"S":"b"},"sk":{"N":"1"}}']).code).toBe(0);
  await open(page, `/resources/dynamodb/tables/${table}?tab=items`, table);
  await expect(page.getByTestId("items-card").locator("tbody tr")).toHaveCount(1);
  await page.getByTestId("item-create").click();
  const dialog = page.getByTestId("item-dialog");
  await dialog.getByTestId("item-json").fill('{"pk": "a", "sk": "x"}');
  await expect(dialog.getByTestId("item-wrong")).toContainText("sk is a key of type N");
  await dialog.getByTestId("item-json").fill('{"pk": "a", "sk": 2, "status": "new"}');
  await dialog.getByTestId("item-save").click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByTestId("items-card").locator("tbody tr")).toHaveCount(2);
  await page.getByTestId("mode-query").click();
  await page.getByTestId("q-pk").fill("a");
  await page.getByTestId("items-run").click();
  const rows = page.getByTestId("items-card").locator("tbody tr");
  await expect(rows).toHaveCount(1);
  await rows.first().click();
  const sheet = page.getByTestId("item-sheet");
  await expect(sheet.getByTestId("item-json-view")).toContainText('"status": "new"');
  await sheet.getByTestId("item-edit").click();
  await page.getByTestId("item-dialog").getByTestId("item-json").fill('{"pk": "a", "sk": 2, "status": "shipped"}');
  await page.getByTestId("item-dialog").getByTestId("item-save").click();
  await expect(sheet).toHaveCount(0);
  await expect(rows.first()).toContainText("shipped");
  await rows.first().click();
  await sheet.getByTestId("item-delete").click();
  await page.getByTestId("item-delete-dialog-submit").click();
  await expect(sheet).toHaveCount(0);
  await expect(rows).toHaveCount(0);
  const left = JSON.parse(cli(["dynamodb", "scan", "--table-name", table]).out) as { Count: number };
  expect(left.Count).toBe(1);
  for (const op of ["PutItem", "Query", "DeleteItem"]) expect((await mine(page, op)).some((c) => c.outcome === "ok"), op).toBe(true);
});
