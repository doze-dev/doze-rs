// The resource pages of their own against a real doze-aws: a bucket's folders, an upload, an object's drawer, a download, a
// delete; a table's items created, scanned, queried, edited and deleted. Each call is the console's, and Live has it. (A queue's
// pages are in sqs-boards.spec.ts.)
import { calls, CONSOLE, expect, open, test, unique } from "./fixtures";

const mine = async (page: Parameters<typeof calls>[0], action: string) => (await calls(page, CONSOLE)).calls.filter((c) => c.caller === "console" && c.action === action);

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
  // A query that returned none says so, and does not tell to choose an item.
  await expect(page.getByTestId("items-card")).toContainText("No items match");
  await expect(page.getByTestId("items-card")).not.toContainText("Choose an item");
  const left = JSON.parse(cli(["dynamodb", "scan", "--table-name", table]).out) as { Count: number };
  expect(left.Count).toBe(1);
  for (const op of ["PutItem", "Query", "DeleteItem"]) expect((await mine(page, op)).some((c) => c.outcome === "ok"), op).toBe(true);
});
