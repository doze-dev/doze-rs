// The resource pages of their own against a real doze-aws: a table's items created, scanned, queried, edited and deleted. Each
// call is the console's, and Live has it. (A queue's pages are in sqs-boards.spec.ts, a bucket's in s3-boards.spec.ts.)
import { calls, CONSOLE, expect, open, test, unique } from "./fixtures";

const mine = async (page: Parameters<typeof calls>[0], action: string) => (await calls(page, CONSOLE)).calls.filter((c) => c.caller === "console" && c.action === action);

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
