// Screenshots of the resource pages of their own (a queue's messages, settings and redrive; a bucket's objects; a table's items)
// for a person to look at: a real doze-aws of its own (ports from the environment, else 42210 and 42211), resources made with
// the real AWS CLI, then each page, drawer and dialog, light and dark, in each browser.
//   bun e2e/deep-shots.ts <out dir> [firefox,webkit,chromium]
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws, start } from "./server";

const [out = "deep-shots", only = "firefox,webkit,chromium"] = process.argv.slice(2);
const listen = Number(process.env.DOZE_AWS_SHOTS_PORT ?? 42210);
const server = await start(listen, listen + 1);
const engines = { firefox, webkit, chromium };

function run(args: string[]) {
  const r = aws(server.endpoint, args, { accessKey: "AKIALARDERAPP0000001" });
  if (r.code !== 0) console.warn(`aws ${args.join(" ")}: ${r.out}`);
  return r;
}

try {
  const acct = server.account;
  const q = (name: string) => `${server.endpoint}/${acct}/${name}`;
  run(["sqs", "create-queue", "--queue-name", "pick-lists-dlq"]);
  const policy = JSON.stringify({ deadLetterTargetArn: `arn:aws:sqs:us-east-1:${acct}:pick-lists-dlq`, maxReceiveCount: 50 });
  run(["sqs", "create-queue", "--queue-name", "pick-lists", "--attributes", JSON.stringify({ RedrivePolicy: policy })]);
  for (const [i, sku] of ["DA-MLK-1", "FV-APL-G", "FV-TOM-V"].entries()) {
    run(["sqs", "send-message", "--queue-url", q("pick-lists"), "--message-body", JSON.stringify({ order_id: `ord-2026110${i}-03${i}2`, sku, qty: i + 1 }), "--message-attributes", `{"source":{"DataType":"String","StringValue":"checkout"}}`]);
  }
  for (const id of ["ord-0371", "ord-0388"]) run(["sqs", "send-message", "--queue-url", q("pick-lists-dlq"), "--message-body", JSON.stringify({ order_id: id, error: "stock lookup timed out" })]);
  run(["sqs", "create-queue", "--queue-name", "stock-moves.fifo", "--attributes", '{"FifoQueue":"true","ContentBasedDeduplication":"true"}']);

  run(["s3api", "create-bucket", "--bucket", "product-images"]);
  run(["s3api", "put-bucket-versioning", "--bucket", "product-images", "--versioning-configuration", "Status=Enabled"]);
  const dir = join(tmpdir(), `doze-deep-shots-${process.pid}`);
  mkdirSync(dir, { recursive: true });
  const file = join(dir, "img.jpg");
  writeFileSync(file, Buffer.alloc(184_213, 7));
  for (const key of ["sku-DA-MLK-1.jpg", "sku-FV-APL-G.jpg", "sku-FV-TOM-V.jpg", "banners/hero.jpg", "banners/autumn.jpg", "catalog/2026/index.json"]) {
    run(["s3api", "put-object", "--bucket", "product-images", "--key", key, "--body", file, "--content-type", key.endsWith(".json") ? "application/json" : "image/jpeg", "--metadata", "uploaded-by=catalog", "--tagging", "team=catalog"]);
  }
  run(["s3api", "put-object", "--bucket", "product-images", "--key", "sku-FV-TOM-V.jpg", "--body", file, "--content-type", "image/jpeg"]);

  const table = { TableName: "orders-table", BillingMode: "PAY_PER_REQUEST", AttributeDefinitions: [{ AttributeName: "order_id", AttributeType: "S" }, { AttributeName: "created_at", AttributeType: "S" }, { AttributeName: "status", AttributeType: "S" }], KeySchema: [{ AttributeName: "order_id", KeyType: "HASH" }, { AttributeName: "created_at", KeyType: "RANGE" }], GlobalSecondaryIndexes: [{ IndexName: "by-status", KeySchema: [{ AttributeName: "status", KeyType: "HASH" }], Projection: { ProjectionType: "ALL" } }] };
  const tfile = join(dir, "table.json");
  writeFileSync(tfile, JSON.stringify(table));
  run(["dynamodb", "create-table", "--cli-input-json", `file://${tfile}`]);
  const statuses = ["shipped", "packed", "shipped", "new", "shipped", "cancelled"];
  statuses.forEach((s, i) => {
    const item = { order_id: { S: `ord-2026110${7 + (i > 3 ? 1 : 0)}-03${70 + i}` }, created_at: { S: `2026-11-0${7 + (i > 3 ? 1 : 0)}T1${i}:11:36Z` }, customer_id: { S: `cus-0${300 + i * 7}` }, status: { S: s }, total: { N: (19.25 + i * 12.4).toFixed(2) }, lines: { L: [{ M: { sku: { S: "FV-APL-G" }, qty: { N: String(i + 1) } } }] } };
    run(["dynamodb", "put-item", "--table-name", "orders-table", "--item", JSON.stringify(item)]);
  });
  await new Promise((r) => setTimeout(r, 500));

  async function settle(p: Page, title: string) {
    await p.locator("h1", { hasText: title }).first().waitFor();
    await p.evaluate(() => document.fonts.ready);
    await p.waitForTimeout(600);
  }

  for (const name of only.split(",") as (keyof typeof engines)[]) {
    const b = await engines[name].launch();
    mkdirSync(`${out}/${name}`, { recursive: true });
    for (const mode of ["light", "dark"] as const) {
      const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
      await ctx.addInitScript((m) => localStorage.setItem("doze-aws.theme", m), mode);
      const p = await ctx.newPage();
      const shot = (what: string, fullPage = false) => p.screenshot({ path: `${out}/${name}/${what}.${mode}.png`, fullPage });
      const page = async (path: string, title: string, what: string, full = true) => {
        await p.goto(`${server.console}${path}`);
        await settle(p, title);
        await shot(what, full);
      };
      const overlay = async (open: () => Promise<void>, testId: string, what: string) => {
        await open();
        await p.getByTestId(testId).waitFor();
        await p.waitForTimeout(500);
        await shot(what);
        await p.keyboard.press("Escape");
        await p.getByTestId(testId).waitFor({ state: "detached" });
      };
      await page("/resources/sqs/queues/pick-lists?tab=messages", "pick-lists", "sqs-messages");
      await p.getByTestId("peek").click();
      await p.getByTestId("peeked").locator("tbody tr").first().waitFor();
      await p.waitForTimeout(300);
      await shot("sqs-peeked", true);
      await overlay(() => p.getByTestId("peeked").locator("tbody tr").first().click(), "message-sheet", "sqs-message");
      await overlay(() => p.getByTestId("purge-open").click(), "purge-dialog", "sqs-purge");
      await page("/resources/sqs/queues/stock-moves.fifo?tab=messages", "stock-moves.fifo", "sqs-fifo", false);
      await page("/resources/sqs/queues/pick-lists?tab=settings", "pick-lists", "sqs-settings");
      await page("/resources/sqs/queues/pick-lists-dlq?tab=redrive", "pick-lists-dlq", "sqs-redrive");
      await p.getByText("To another queue").click();
      await p.getByTestId("redrive-queue").fill("pick-lists");
      await p.getByTestId("redrive-start").click();
      await p.waitForTimeout(600);
      await p.getByTestId("moves-refresh").click();
      await p.getByTestId("moves-card").locator("tbody tr").first().waitFor();
      await p.waitForTimeout(400);
      await shot("sqs-redrive-done", true);
      await page("/resources/s3/buckets/product-images?tab=objects", "product-images", "s3-objects");
      await overlay(() => p.getByTestId("objects-card").locator("tbody tr", { hasText: "sku-FV-TOM-V.jpg" }).click(), "object-sheet", "s3-object");
      await overlay(() => p.getByTestId("upload-open").click(), "upload-dialog", "s3-upload");
      await overlay(() => p.getByTestId("folder-open").click(), "folder-dialog", "s3-folder");
      await page(`/resources/s3/buckets/product-images?tab=objects&p=${encodeURIComponent("banners/")}`, "product-images", "s3-folder-view", false);
      await page("/resources/dynamodb/tables/orders-table?tab=items", "orders-table", "ddb-items");
      await overlay(() => p.getByTestId("items-card").locator("tbody tr").first().click(), "item-sheet", "ddb-item");
      await overlay(() => p.getByTestId("item-create").click(), "item-dialog", "ddb-item-create");
      await p.getByTestId("mode-query").click();
      await p.getByTestId("q-pk").fill("ord-20261107-0372");
      await p.getByTestId("items-run").click();
      await p.waitForTimeout(800);
      await shot("ddb-query", true);
      await page("/resources/sqs/queues/pick-lists", "pick-lists", "sqs-details-tabs", false);
      await ctx.close();
    }
    await b.close();
  }
} finally {
  await server.stop();
}
