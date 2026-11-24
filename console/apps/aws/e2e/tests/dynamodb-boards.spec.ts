// The DynamoDB boards (29 of the canvas), each reached against a real doze-aws (one per browser, with its own data, seeded as the
// boards' "larder" world with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside
// the board. The tests run in order: each leaves the world as the next board needs it.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { start, type Running, aws } from "../server";
import { keyParam } from "../../src/pages/resources/dynamodb/item-rows";
import { seedLarder } from "./dynamodb-seed";
import { expectCopy } from "./callpanel-kit";
import { expect, expectNoErrorAnswers, test } from "./fixtures";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-dynamodb");
const BASE = Number(process.env.E2E_BASE_PORT ?? 44800);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });

let server: Running;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

/** Photographs the page as the board is: light, then dark, in Chromium only. */
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
const T = "/resources/dynamodb/tables";
const code = (page: Page) => page.getByTestId("call-panel-code");

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  seedLarder(server.endpoint);
});
test.afterAll(async () => {
  await server.stop();
});

test("dynamodb__tables-list and table-delete: every table with its keys, indexes and capacity; Delete asks, and is cancelled", async ({ page }) => {
  await go(page, T, "Tables");
  await expect(page.getByTestId("list-sentence")).toContainText("3 tables in us-east-1 hold about 13 items.");
  const rows = page.getByRole("table", { name: "Tables" }).locator("tbody tr[data-row]");
  await expect(rows).toHaveCount(3);
  await expect(rows.filter({ hasText: "orders-table" })).toContainText("created_at");
  await expect(rows.filter({ hasText: "deliveries" })).toContainText("Provisioned");
  await expect(rows.filter({ hasText: "deliveries" })).toContainText("5 read, 5 write units");
  await expect(rows.filter({ hasText: "orders-table" })).toContainText("On-demand");
  await expect(page.getByTestId("table-delete")).toBeDisabled();
  await page.getByTestId("select-deliveries").click();
  await expect(page.getByTestId("list-sentence")).toContainText("deliveries is ticked.");
  await expect(page.getByTestId("list-sentence")).toContainText("doze-aws counts live");
  await shot(page, "dynamodb__tables-list");
  await page.getByTestId("table-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("You are about to delete deliveries and its 6 items.");
  await expect(page.getByTestId("delete-dialog")).toContainText("(deliveries has none)");
  await expect(page.getByTestId("delete-dialog")).toContainText("No way back.");
  await expect(page.getByTestId("delete-protection")).toContainText("Deletion protection is off for this table.");
  await shot(page, "dynamodb__table-delete");
  await page.getByTestId("delete-dialog-cancel").click();
  await expect(page.getByTestId("delete-dialog")).toHaveCount(0);
});

test("dynamodb__table-create and table-create-exists: a name and a key, the call, and a name that is taken", async ({ page, refusals }) => {
  // The name that is taken is DynamoDB's refusal, shown on the form.
  refusals.allow(/400 POST .*\/dynamodb\/tables\/op\/CreateTable$/);
  await go(page, `${T}?do=create`, "Create table");
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("Tables");
  // The optional sort key carries no placeholder that reads as a value.
  await expect(page.getByTestId("table-sk")).toHaveAttribute("placeholder", "");
  await page.getByTestId("table-name").fill("deliveries");
  await expect(page.getByText("orders-table and search-index already exist here.").or(page.getByText("already exist here."))).toBeVisible();
  await page.getByTestId("table-pk").fill("delivery_id");
  await expect(code(page)).toContainText('"BillingMode": "PAY_PER_REQUEST"');
  await expect(code(page)).toContainText('"AttributeName": "delivery_id"');
  await expectCopy(page, "cli", "dynamodb create-table");
  await shot(page, "dynamodb__table-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("create-refused")).toContainText("The table was not created.");
  await expect(page.getByTestId("create-refused")).toContainText("ResourceInUseException");
  await expect(page.getByTestId("create-refused")).toContainText("Retrying the same request will not help.");
  await expect(page.getByTestId("section-Table details")).toContainText("CreateTable refused this name: ResourceInUseException.");
  // As the board draws it: the details alone, without the name hint or the settings card.
  await expect(page.getByTestId("section-Table details")).not.toContainText("already exist here");
  await expect(page.getByTestId("section-Table settings")).toHaveCount(0);
  await shot(page, "dynamodb__table-create-exists");
});

test("dynamodb__table-create-custom and table-created: capacity, encryption and tags, then the list that says it is Active", async ({ page }) => {
  await go(page, `${T}?do=create`, "Create table");
  await page.getByTestId("table-name").fill("stock-moves");
  await page.getByTestId("table-pk").fill("sku");
  await page.getByTestId("table-sk").fill("moved_at");
  await page.getByTestId("settings-custom").click();
  await expect(page.getByTestId("section-Read/write capacity settings")).toContainText("Stored, not applied locally.");
  await expect(page.getByTestId("section-Encryption at rest")).toContainText("Owned by Amazon DynamoDB");
  await page.getByTestId("create-tags-key-0").fill("app");
  await page.getByTestId("create-tags-value-0").fill("larder");
  await page.getByTestId("create-tags-add").click();
  await page.getByTestId("create-tags-key-1").fill("owner");
  await page.getByTestId("create-tags-value-1").fill("fulfilment");
  await expect(code(page)).toContainText('"Key": "owner"');
  await expect(code(page)).toContainText('"KeyType": "RANGE"');
  await shot(page, "dynamodb__table-create-custom");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("stock-moves was created and is Active.");
  await expect(page.getByTestId("created-banner")).toContainText("here it is ready at once");
  await expect(page.getByRole("table", { name: "Tables" }).locator("tbody tr[data-row]")).toHaveCount(4);
  await shot(page, "dynamodb__table-created");
  expect(cli(["dynamodb", "list-tags-of-resource", "--resource-arn", `arn:aws:dynamodb:us-east-1:${server.account}:table/stock-moves`])).toContain("fulfilment");
});

test("a table that is not there: one page for a missing name, with the way back and the service's code", async ({ page, refusals }) => {
  // A page of what is not there asks the service, which refuses.
  refusals.allow(/400 GET .*\/dynamodb\/tables\/no-such-table$/);
  refusals.allow(/400 POST .*\/dynamodb\/tables\/no-such-table\/op\/DescribeTable$/);
  await page.goto(`${server.console}${T}/no-such-table`);
  await expect(page.locator("h1").first()).toHaveText("Table not found");
  const err = page.getByTestId("detail-error");
  await expect(err).toContainText("There is no table named no-such-table");
  await expect(err).toContainText("There is no table of that name in this account and Region.");
  await expect(err).toContainText("ResourceNotFoundException");
  await expect(err).not.toContainText("Check that doze-aws is still running");
  // Nothing to pin on a page of what is not there.
  await expect(page.getByTestId("pin-toggle")).toHaveCount(0);
  await expect(page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link")).toContainText(["Resources", "DynamoDB", "Tables"]);
  await shot(page, "dynamodb__table-missing");
  // The same page behind the item forms and the settings forms.
  await page.goto(`${server.console}${T}/no-such-table?do=explore`);
  await expect(page.locator("h1").first()).toHaveText("Table not found");
  await err.getByRole("link", { name: "Back to tables" }).click();
  await expect(page.locator("h1").first()).toHaveText("Tables");
  // A read that failed otherwise keeps Try again (not covered here: it needs the server to fail).
});

test("dynamodb__table-detail: the sentence, what it is wired to, and the fields of DescribeTable", async ({ page }) => {
  await go(page, T, "Tables");
  await page.getByRole("link", { name: "orders-table" }).click();
  await expect(page.locator("h1").first()).toHaveText("orders-table");
  await expect(page.getByTestId("detail-badge")).toHaveText("ACTIVE");
  await expect(page.getByTestId("detail-sentence")).toContainText("orders-table holds 7 items. One global secondary index, by-customer, is Active. No DynamoDB stream is on.");
  await expect(page.getByTestId("wired-strip")).toContainText("by-customer index");
  await expect(page.getByTestId("fact-Partition key")).toHaveText("order_id (String)");
  await expect(page.getByTestId("fact-Sort key")).toHaveText("created_at (String)");
  await expect(page.getByTestId("fact-Capacity mode")).toHaveText("On-demand");
  await expect(page.getByTestId("fact-ItemCount")).toHaveText("7");
  await expect(page.getByTestId("fact-Encryption type")).toContainText("Owned by Amazon DynamoDB");
  await expect(page.getByTestId("fact-Encryption type")).toContainText("Stored, not applied locally.");
  await expect(page.getByTestId("verb-explore")).toHaveText("Explore table items");
  await expect(page.getByTestId("recent-calls")).toContainText("CreateTable");
  // A batch names its tables only inside RequestItems: the call is still the table's (the seed wrote its items with one BatchWriteItem).
  await expect(page.getByTestId("recent-calls")).toContainText("BatchWriteItem");
  await shot(page, "dynamodb__table-detail");
});

test("dynamodb__table-indexes, index-create, index-created and index-delete: the index, a new one, and its removal", async ({ page, refusals }) => {
  await go(page, `${T}/orders-table?tab=indexes`, "orders-table");
  await expect(page.getByTestId("global-indexes")).toContainText("by-customer");
  await expect(page.getByTestId("global-indexes")).toContainText("customer_id");
  await expect(page.getByTestId("global-indexes")).toContainText("ALL");
  await expect(page.getByTestId("local-indexes")).toContainText("None. A local index shares the table's partition key");
  await shot(page, "dynamodb__table-indexes");
  await page.getByTestId("index-create-open").click();
  await expect(page.locator("h1").first()).toHaveText("Create global secondary index");
  await page.getByTestId("index-name").fill("by-status");
  await expect(page.getByText("by-customer is the only other index.")).toBeVisible();
  await page.getByTestId("index-pk").fill("status");
  await page.getByTestId("index-sk").fill("created_at");
  await expect(code(page)).toContainText('"IndexName": "by-status"');
  await expect(code(page)).toContainText('"ProjectionType": "ALL"');
  await expect(page.getByTestId("index-note")).toContainText("one index can be created or deleted per request");
  await shot(page, "dynamodb__index-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("result-banner")).toContainText("by-status was created from the table's 7 items;");
  await expect(page.getByTestId("global-indexes")).toContainText("by-status");
  await shot(page, "dynamodb__index-created");
  await page.getByTestId("index-delete-by-status").click();
  await expect(page.getByTestId("index-delete-dialog")).toContainText("Delete index by-status?");
  await expect(page.getByTestId("index-delete-call-code")).toContainText('"Delete"');
  await page.getByTestId("index-delete-dialog-cancel").click();
  await page.getByTestId("index-delete-by-customer").click();
  await expect(page.getByTestId("index-delete-dialog")).toContainText("Delete index by-customer?");
  await expect(page.getByTestId("index-delete-dialog")).toContainText("The items in orders-table are not touched.");
  await shot(page, "dynamodb__index-delete");
  await page.getByTestId("index-delete-dialog-cancel").click();
  // The index the page made is dropped again, so the table is as the boards have it.
  await page.getByTestId("index-delete-by-status").click();
  await page.getByTestId("index-delete-dialog-submit").click();
  await expect(page.getByTestId("result-banner")).toContainText("by-status was deleted.");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("global-indexes")).not.toContainText("by-status");
});

test("dynamodb__indexes-none: a table with no index says what an index is for", async ({ page }) => {
  await go(page, `${T}/deliveries?tab=indexes`, "deliveries");
  await expect(page.getByTestId("indexes-none")).toContainText("No global secondary indexes");
  await expect(page.getByTestId("indexes-none")).toContainText("deliveries is queried by its primary key only.");
  await expect(page.getByTestId("indexes-none").locator("span.rounded-xl svg")).toHaveCount(1);
  await shot(page, "dynamodb__indexes-none");
});

test("dynamodb__table-monitor: Contributor Insights of the table and its index, turned on", async ({ page }) => {
  await go(page, `${T}/orders-table?tab=monitor`, "orders-table");
  await expect(page.getByTestId("insights-orders-table")).toContainText("DISABLED");
  await expect(page.getByTestId("insights-by-customer")).toContainText("DISABLED");
  await expect(page.getByTestId("insights")).toContainText("Stored, not applied locally.");
  await shot(page, "dynamodb__table-monitor");
  await page.getByTestId("insights-toggle").click();
  await expect(page.getByTestId("insights-orders-table")).toContainText("ENABLED");
  await expect(page.getByTestId("insights-toggle")).toHaveText("Turn off CloudWatch Contributor Insights");
  await page.getByTestId("insights-toggle").click();
  await expect(page.getByTestId("insights-by-customer")).toContainText("DISABLED");
});

test("dynamodb__table-exports and exports-none: the stream, an export to S3 and a Kinesis destination, each turned on", async ({ page }) => {
  await go(page, `${T}/orders-table?tab=exports`, "orders-table");
  await expect(page.getByTestId("fact-StreamEnabled")).toContainText("Off");
  await expect(page.getByTestId("exports-none")).toContainText("orders-table has not been exported.");
  await expect(page.getByTestId("exports-none").locator("span.rounded-xl svg")).toHaveCount(1);
  await shot(page, "dynamodb__table-exports");
  await page.getByTestId("stream-on-open").click();
  await expect(page.getByTestId("stream-on-call-code")).toContainText('"StreamViewType": "NEW_AND_OLD_IMAGES"');
  await page.getByTestId("stream-on-save").click();
  await expect(page.getByTestId("exports-done")).toContainText("The DynamoDB stream of orders-table is on.");
  await expect(page.getByTestId("fact-StreamViewType")).toHaveText("NEW_AND_OLD_IMAGES");
  await expect(page.getByTestId("fact-LatestStreamArn")).toContainText("arn:aws:dynamodb");
  // An export needs point-in-time recovery.
  await page.getByTestId("export-open").click();
  await expect(page.getByTestId("pitr-off")).toContainText("Point-in-time recovery is off.");
  await page.getByTestId("pitr-on").click();
  await expect(page.getByTestId("pitr-off")).toHaveCount(0);
  await page.getByRole("combobox", { name: "Choose a bucket" }).click();
  await page.getByRole("option", { name: "larder-exports" }).click();
  await expect(page.getByTestId("export-dialog-call-code")).toContainText('"S3Bucket": "larder-exports"');
  await page.getByTestId("export-dialog-save").click();
  await expect(page.getByTestId("exports-done")).toContainText("The export");
  await expect(page.getByRole("table", { name: "Exports" })).toContainText("COMPLETED");
  // Kinesis.
  await page.getByTestId("kinesis-on-open").click();
  await page.getByRole("combobox", { name: "Choose a stream" }).click();
  await page.getByRole("option", { name: "orders-changes" }).click();
  await page.getByTestId("kinesis-on-save").click();
  await expect(page.getByTestId("kinesis-off-open")).toBeVisible();
  await expect(page.getByTestId("kinesis-card")).toContainText("orders-changes");
  await go(page, `${T}/search-index?tab=exports`, "search-index");
  await expect(page.getByTestId("exports-none")).toContainText("No exports");
  await shot(page, "dynamodb__exports-none");
});

test("dynamodb__table-settings, capacity-edit and ttl-edit: capacity, Time to Live, encryption, protection and tags", async ({ page }) => {
  await go(page, `${T}/orders-table?tab=settings`, "orders-table");
  await expect(page.getByTestId("capacity")).toContainText("On-demand");
  await expect(page.getByTestId("capacity")).toContainText("Stored, not applied locally.");
  await expect(page.getByTestId("fact-TimeToLiveStatus")).toContainText("DISABLED");
  await expect(page.getByTestId("encryption")).toContainText("Owned by Amazon DynamoDB");
  await expect(page.getByTestId("protection")).toContainText("Currently off for orders-table.");
  await expect(page.getByTestId("table-tags")).toContainText("owner");
  await shot(page, "dynamodb__table-settings");
  await page.getByTestId("capacity-edit-open").click();
  await expect(page.locator("h1").first()).toHaveText("Edit read/write capacity");
  await page.getByTestId("capacity-mode-PROVISIONED").click();
  await page.getByTestId("capacity-read").fill("20");
  await page.getByTestId("capacity-write").fill("10");
  await expect(code(page)).toContainText('"ReadCapacityUnits": 20');
  await expect(code(page)).toContainText('"BillingMode": "PROVISIONED"');
  await shot(page, "dynamodb__capacity-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("result-banner")).toContainText("Read/write capacity updated.");
  await go(page, `${T}/orders-table?tab=settings`, "orders-table");
  await expect(page.getByTestId("capacity")).toContainText("20");
  await page.getByTestId("ttl-edit-open").click();
  await expect(page.locator("h1").first()).toHaveText("Turn on Time to Live (TTL)");
  await page.getByTestId("ttl-attribute").fill("expires_at");
  await expect(code(page)).toContainText('"AttributeName": "expires_at"');
  await shot(page, "dynamodb__ttl-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("result-banner")).toContainText("Time to Live is on for orders-table.");
  await expect(page.getByTestId("fact-TimeToLiveStatus")).toContainText("ENABLED");
  await expect(page.getByTestId("fact-AttributeName")).toHaveText("expires_at");
  // Deletion protection, then a tag.
  await page.getByTestId("protection-check").click();
  await page.getByTestId("protection-save").click();
  await expect(page.getByTestId("settings-saved")).toContainText("Deletion protection is on for orders-table.");
  await page.getByTestId("protection-check").click();
  await page.getByTestId("protection-save").click();
  await expect(page.getByTestId("protection")).toContainText("Currently off for orders-table.");
  await page.getByTestId("tag-add").click();
  await page.getByTestId("tag-key").fill("tier");
  await page.getByTestId("tag-value").fill("gold");
  await page.getByTestId("tag-save").click();
  await expect(page.getByTestId("table-tags")).toContainText("gold");
  await page.getByTestId("tag-delete-tier").click();
  await expect(page.getByTestId("table-tags")).not.toContainText("gold");
});

test("dynamodb__table-tags-none: a table with no tags", async ({ page }) => {
  await go(page, `${T}/deliveries?tab=settings`, "deliveries");
  await expect(page.getByTestId("tags-none")).toContainText("No tags");
  await expect(page.getByTestId("tags-none")).toContainText("deliveries has no tags.");
  await shot(page, "dynamodb__table-tags-none");
});

test("dynamodb__items-explore, items-scan and items-none: a query by key and index, a scan with a filter, and a query that finds nothing", async ({ page }) => {
  await go(page, `${T}/orders-table`, "orders-table");
  await page.getByTestId("verb-explore").click();
  await expect(page.locator("h1").first()).toHaveText("Explore table items");
  await page.getByTestId("mode-query").click();
  await page.getByTestId("items-source").click();
  await page.getByRole("option", { name: "orders-table - by-customer (global index)" }).click();
  await page.getByTestId("q-pk").fill("cus-1042");
  await page.getByTestId("q-sortop").click();
  await page.getByRole("option", { name: "begins_with" }).click();
  await page.getByTestId("q-sk").fill("2026-11");
  await expect(code(page)).toContainText('"IndexName": "by-customer"');
  await expect(code(page)).toContainText('begins_with(#sk, :sk)');
  await page.getByTestId("items-run").click();
  const rows = page.getByRole("table", { name: "Items" }).locator("tbody tr[data-row]");
  await expect(rows).toHaveCount(3);
  await expect(page.getByTestId("items-card")).toContainText("Items returned (3)");
  await expect(page.getByTestId("items-card")).toContainText("Everything the query found for customer cus-1042.");
  await expect(page.getByTestId("items-note")).toContainText("there is nothing to continue");
  // A status is its word with a dot of the colour it means (delivered is good, placed is waiting).
  await expect(rows.nth(0).locator("[data-tone]")).toHaveAttribute("data-tone", "ok");
  await expect(rows.nth(1).locator("[data-tone]")).toHaveAttribute("data-tone", "warn");
  await expect(rows.nth(1).locator("[data-tone]")).toHaveText("placed");
  await rows.first().getByRole("checkbox").click();
  await shot(page, "dynamodb__items-explore");
  await page.getByTestId("mode-scan").click();
  await page.getByTestId("items-source").click();
  await page.getByRole("option", { name: "orders-table", exact: true }).click();
  await page.getByTestId("s-filter").fill("#s = :s");
  await page.getByTestId("s-names").fill('{"#s": "status"}');
  await page.getByTestId("s-values").fill('{":s": {"S": "shipped"}}');
  await page.getByTestId("items-run").click();
  await expect(rows).toHaveCount(4);
  await expect(page.getByTestId("items-card")).toContainText("Scanned 7 items; 4 match the filter.");
  await shot(page, "dynamodb__items-scan");
  await page.getByTestId("mode-query").click();
  await page.getByTestId("items-source").click();
  await page.getByRole("option", { name: "orders-table - by-customer (global index)" }).click();
  await page.getByTestId("q-pk").fill("cus-9999");
  await page.getByTestId("items-run").click();
  await expect(page.getByTestId("items-none")).toContainText("No items returned");
  await expect(page.getByTestId("items-card")).toContainText("Items returned (0)");
  await expect(page.getByTestId("items-none")).toContainText("The query found no items for this key. Change the key or the index.");
  await shot(page, "dynamodb__items-none");
});

test("dynamodb__item-create and item-edit: an item as rows, its keys fixed, saved with PutItem", async ({ page }) => {
  await go(page, `${T}/orders-table?do=item-create`, "Create item");
  // Nothing typed: the form has not been wrong about anything, so the footer says nothing yet.
  await expect(page.getByTestId("form-submit")).toBeDisabled();
  await expect(page.getByText("a key cannot be empty")).toHaveCount(0);
  await expect(page.getByTestId("attr-order_id").getByTestId("attr-name")).toBeDisabled();
  await expect(page.getByTestId("attr-created_at")).toContainText("Sort key");
  await page.getByTestId("attr-order_id").getByTestId("attr-value").fill("ord-20261108-0431");
  await page.getByTestId("attr-created_at").getByTestId("attr-value").fill("2026-11-08T08:14:07Z");
  await page.getByTestId("attr-add").click();
  await page.getByTestId("attr-new").getByTestId("attr-name").fill("customer_id");
  await page.getByTestId("attr-customer_id").getByTestId("attr-value").fill("cus-0556");
  await page.getByTestId("attr-add").click();
  await page.getByTestId("attr-new").getByTestId("attr-name").fill("total");
  await page.getByTestId("attr-total").getByTestId("attr-type").click();
  await page.getByRole("option", { name: "Number", exact: true }).click();
  await page.getByTestId("attr-total").getByTestId("attr-value").fill("24.90");
  await expect(code(page)).toContainText('"N": "24.90"');
  await expect(code(page)).toContainText('"S": "ord-20261108-0431"');
  await shot(page, "dynamodb__item-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("item-banner")).toContainText("Item created.");
  expect(cli(["dynamodb", "get-item", "--table-name", "orders-table", "--key", JSON.stringify({ order_id: { S: "ord-20261108-0431" }, created_at: { S: "2026-11-08T08:14:07Z" } })])).toContain("24.9");
  // Edit the first order of cus-1042.
  const key = JSON.stringify({ order_id: { S: "ord-20261106-0288" }, created_at: { S: "2026-11-06T19:20:51Z" } });
  await go(page, `${T}/orders-table?do=item-edit&key=${keyParam(JSON.parse(key))}`, "Edit item");
  await expect(page.getByTestId("attr-order_id").getByTestId("attr-value")).toBeDisabled();
  await expect(page.getByTestId("attr-status").getByTestId("attr-value")).toHaveValue("delivered");
  await page.getByTestId("attr-add").click();
  await page.getByTestId("attr-new").getByTestId("attr-name").fill("delivered_at");
  await page.getByTestId("attr-delivered_at").getByTestId("attr-value").fill("2026-11-07T10:42:18Z");
  await expect(code(page)).toContainText('"delivered_at"');
  await shot(page, "dynamodb__item-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("item-banner")).toContainText("Item saved.");
  expect(cli(["dynamodb", "get-item", "--table-name", "orders-table", "--key", key])).toContain("delivered_at");
});

test("dynamodb__item-delete: the ticked item is named by its key and deleted", async ({ page, refusals }) => {
  await go(page, `${T}/orders-table?do=explore`, "Explore table items");
  await page.getByTestId("mode-query").click();
  await page.getByTestId("items-source").click();
  await page.getByRole("option", { name: "orders-table - by-customer (global index)" }).click();
  await page.getByTestId("q-pk").fill("cus-1042");
  await page.getByTestId("items-run").click();
  const rows = page.getByRole("table", { name: "Items" }).locator("tbody tr[data-row]");
  await expect(rows).toHaveCount(3);
  await expect(page.getByTestId("item-delete")).toBeDisabled();
  // Wait until the call panel has shown the final call before ticking: the preview of the call lands a moment after the typing, resizes
  // the panel and so moves the table under the pointer. A click whose mouse-down and mouse-up land on different elements is no click
  // (Firefox on a busy machine: Delete stayed disabled). A person waits for "Updating…" to go; so does the test.
  await expect(code(page)).toContainText('"S": "cus-1042"');
  await expect(page.getByTestId("call-panel-settling")).toHaveCount(0);
  await rows.filter({ hasText: "ord-20261108-0412" }).getByRole("checkbox").check();
  await expect(page.getByTestId("item-delete")).toBeEnabled();
  await page.getByTestId("item-delete").click();
  await expect(page.getByTestId("item-delete-dialog")).toContainText("Delete 1 item?");
  await expect(page.getByTestId("item-key")).toContainText("ord-20261108-0412");
  await expect(page.getByTestId("item-key")).toContainText("2026-11-08T07:58:12Z");
  await expect(page.getByTestId("item-delete-dialog")).toContainText("The item also disappears from by-customer.");
  await shot(page, "dynamodb__item-delete");
  await page.getByTestId("item-delete-dialog-submit").click();
  await expect(page.getByTestId("item-banner")).toContainText("Item deleted.");
  await expectNoErrorAnswers(page, refusals);
  expect(cli(["dynamodb", "scan", "--table-name", "orders-table", "--select", "COUNT"])).toContain('"Count": 7');
});

test("dynamodb__partiql and partiql-error: a statement run, and a write that finds no item", async ({ page, refusals }) => {
  // The write that finds no item is DynamoDB's refusal, shown under the statement.
  refusals.allow(/400 POST .*\/dynamodb\/tables\/op\/ExecuteStatement$/);
  await go(page, `${T}?do=partiql`, "PartiQL editor");
  await page.getByTestId("partiql-table").click();
  await page.getByRole("option", { name: "orders-table" }).click();
  await page.getByTestId("partiql-start").click();
  await expect(page.getByTestId("partiql-statement")).toHaveValue('SELECT * FROM "orders-table"');
  await expect(page.getByTestId("partiql-none").locator("span.rounded-xl svg")).toHaveCount(1);
  // The box suggests the statement on four lines, as the board lays it out; a statement may be typed over as many lines as a person likes.
  await expect(page.getByTestId("partiql-statement")).toHaveAttribute("placeholder", /^UPDATE "orders-table"\nSET status = 'cancelled'\nWHERE order_id = '.+'\nAND created_at = '.+'$/);
  await page.getByTestId("partiql-statement").fill(`UPDATE "orders-table"\nSET status = 'cancelled'\nWHERE order_id = 'ord-20261108-0499'\nAND created_at = '2026-11-08T08:01:00Z'`);
  await expect(code(page)).toContainText("ExecuteStatement");
  await expect(page.getByTestId("partiql-none")).toContainText("Nothing run yet");
  await shot(page, "dynamodb__partiql");
  await page.getByTestId("partiql-run").click();
  await expect(page.getByTestId("partiql-error")).toContainText("Nothing was changed.");
  await expect(page.getByTestId("partiql-error")).toContainText("ConditionalCheckFailedException");
  // The service's own words stay, with what they mean beside them.
  await expect(page.getByTestId("partiql-plain")).toHaveText("No item in orders-table has that order_id and created_at, so there was nothing to update.");
  await expect(page.getByTestId("partiql-raw")).toContainText("The service said: The conditional request failed");
  await expect(page.getByTestId("call-panel-came-back")).toHaveCount(0);
  await expect(page.getByTestId("partiql-outcomes")).toContainText("DuplicateItemException");
  await shot(page, "dynamodb__partiql-error");
  await page.getByTestId("partiql-table").click();
  await page.getByRole("option", { name: "deliveries" }).click();
  await page.getByTestId("partiql-statement").fill(`SELECT order_id, status FROM "orders-table" WHERE order_id = 'ord-20261107-0372'`);
  await page.getByTestId("partiql-run").click();
  await expect(page.getByRole("table", { name: "Results" })).toContainText("shipped");
  // Attribute names are case-sensitive and are shown as written; the picker follows the table the statement names.
  await expect(page.getByRole("table", { name: "Results" }).getByRole("columnheader", { name: "order_id" })).toBeVisible();
  await expect(page.getByTestId("partiql-table")).toContainText("orders-table");
  await page.getByTestId("view-json").click();
  await expect(page.getByTestId("results-json")).toContainText('"status": "shipped"');
});

test("dynamodb__table-deleted and tables-none: a table and its items are gone, and the last table leaves the empty state", async ({ page, refusals }) => {
  // A table with deletion protection is refused its delete, and the dialog says so.
  refusals.allow(/400 DELETE .*\/dynamodb\/tables\/search-index$/);
  await go(page, T, "Tables");
  await page.getByTestId("select-deliveries").click();
  await page.getByTestId("table-delete").click();
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("deliveries and its 6 items are gone.");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("deleted-banner")).toContainText("here it is removed at once");
  await expect(page.getByRole("table", { name: "Tables" }).locator("tbody tr[data-row]")).toHaveCount(3);
  await shot(page, "dynamodb__table-deleted");
  // Deletion protection makes the service refuse.
  cli(["dynamodb", "update-table", "--table-name", "search-index", "--deletion-protection-enabled"]);
  await page.getByTestId("select-search-index").click();
  await page.getByTestId("table-delete").click();
  await expect(page.getByTestId("delete-protection")).toContainText("Deletion protection is on for this table.");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("delete-dialog-error")).toContainText("DeleteTable was refused");
  await expectNoErrorAnswers(page, refusals);
  await page.getByTestId("delete-dialog-cancel").click();
  cli(["dynamodb", "update-table", "--table-name", "search-index", "--no-deletion-protection-enabled"]);
  for (const t of ["orders-table", "search-index", "stock-moves"]) cli(["dynamodb", "delete-table", "--table-name", t]);
  await go(page, T, "Tables");
  await expect(page.getByTestId("list-empty")).toContainText("No tables");
  await expect(page.getByTestId("list-empty")).toContainText("You have no tables in this region.");
  await shot(page, "dynamodb__tables-none");
});
