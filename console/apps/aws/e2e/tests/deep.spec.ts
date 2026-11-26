// The resource pages of their own against a real doze-aws: a table's items created, scanned, queried, edited and deleted. Each
// call is the console's, and Live has it. (A queue's pages are in sqs-boards.spec.ts, a bucket's in s3-boards.spec.ts.)
import { advance, calls, CONSOLE, expect, expectNoErrorAnswers, open, test, unique } from "./fixtures";

const mine = async (page: Parameters<typeof calls>[0], action: string) => (await calls(page, CONSOLE)).calls.filter((c) => c.caller === "console" && c.action === action);

test("a table's item is created as rows, scanned, queried by its key, edited and deleted", async ({ page, cli, browserName, refusals }) => {
  const table = unique("orders", browserName);
  expect(cli(["dynamodb", "create-table", "--table-name", table, "--billing-mode", "PAY_PER_REQUEST", "--attribute-definitions", "AttributeName=pk,AttributeType=S", "AttributeName=sk,AttributeType=N", "--key-schema", "AttributeName=pk,KeyType=HASH", "AttributeName=sk,KeyType=RANGE"]).code).toBe(0);
  expect(cli(["dynamodb", "put-item", "--table-name", table, "--item", '{"pk":{"S":"b"},"sk":{"N":"1"}}']).code).toBe(0);
  await open(page, `/resources/dynamodb/tables/${table}?do=explore`, "Explore table items");
  const rows = page.getByRole("table", { name: "Items" }).locator("tbody tr[data-row]");
  await expect(rows).toHaveCount(1);
  await page.getByTestId("item-create").click();
  await expect(page.locator("h1")).toHaveText("Create item");
  // A key of the wrong type is said before anything is sent.
  await page.getByTestId("attr-pk").getByTestId("attr-value").fill("a");
  await page.getByTestId("attr-sk").getByTestId("attr-value").fill("x");
  await expect(page.getByTestId("disabled-why")).toContainText("sk: a number is needed.");
  await page.getByTestId("attr-sk").getByTestId("attr-value").fill("2");
  await page.getByTestId("attr-add").click();
  await page.getByTestId("attr-new").getByTestId("attr-name").fill("status");
  await page.getByTestId("attr-status").getByTestId("attr-value").fill("new");
  await advance(page);
  await expect(page.getByTestId("item-banner")).toContainText("Item created.");
  await expect(rows).toHaveCount(2);
  await page.getByTestId("mode-query").click();
  await page.getByTestId("q-pk").fill("a");
  await page.getByTestId("items-run").click();
  await expect(rows).toHaveCount(1);
  await rows.first().getByRole("radio").click();
  await page.getByTestId("item-edit").click();
  await expect(page.locator("h1")).toHaveText("Edit item");
  await page.getByTestId("attr-status").getByTestId("attr-value").fill("shipped");
  await advance(page);
  await expect(page.getByTestId("item-banner")).toContainText("Item saved.");
  await expect(rows.first()).toContainText("shipped");
  await rows.first().getByRole("radio").click();
  await page.getByTestId("item-delete").click();
  await page.getByTestId("item-delete-dialog-submit").click();
  await expect(page.getByTestId("item-banner")).toContainText("Item deleted.");
  await expectNoErrorAnswers(page, refusals);
  await expect(rows).toHaveCount(0);
  // A query that returned none says so, and does not offer to edit or delete.
  await expect(page.getByTestId("items-none")).toContainText("No items returned");
  await expect(page.getByTestId("item-edit")).toHaveCount(0);
  const left = JSON.parse(cli(["dynamodb", "scan", "--table-name", table]).out) as { Count: number };
  expect(left.Count).toBe(1);
  for (const op of ["PutItem", "Query", "DeleteItem"]) expect((await mine(page, op)).some((c) => c.outcome === "ok"), op).toBe(true);
});
