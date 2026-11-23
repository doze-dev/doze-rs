// The world the DynamoDB boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: orders-table (order_id and
// created_at, an index by-customer, two tags), deliveries (provisioned 5 and 5, no index, no tag), search-index (on demand), an S3
// bucket to export to and a Kinesis stream to send changes to. `bun e2e/tests/dynamodb-seed.ts http://127.0.0.1:44800` seeds a running one.
import { aws } from "../server";

type Cli = (args: string[]) => string;

function runner(endpoint: string): Cli {
  return (args) => {
    const r = aws(endpoint, args);
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
}

const s = (v: string) => ({ S: v });
const n = (v: string) => ({ N: v });

/** orders-table's items: three of cus-1042's, one more of other customers placed, four shipped. */
export const ORDERS = [
  { order_id: s("ord-20261106-0288"), created_at: s("2026-11-06T19:20:51Z"), customer_id: s("cus-1042"), status: s("delivered"), total: n("31.60") },
  { order_id: s("ord-20261108-0409"), created_at: s("2026-11-08T07:33:05Z"), customer_id: s("cus-1042"), status: s("placed"), total: n("9.99") },
  { order_id: s("ord-20261108-0412"), created_at: s("2026-11-08T07:58:12Z"), customer_id: s("cus-1042"), status: s("placed"), total: n("42.80") },
  { order_id: s("ord-20261107-0372"), created_at: s("2026-11-07T15:11:36Z"), customer_id: s("cus-0301"), status: s("shipped"), total: n("19.25") },
  { order_id: s("ord-20261107-0385"), created_at: s("2026-11-07T18:40:02Z"), customer_id: s("cus-0556"), status: s("shipped"), total: n("44.90") },
  { order_id: s("ord-20261107-0398"), created_at: s("2026-11-07T21:14:48Z"), customer_id: s("cus-0231"), status: s("shipped"), total: n("67.10") },
  { order_id: s("ord-20261108-0411"), created_at: s("2026-11-08T07:51:40Z"), customer_id: s("cus-0877"), status: s("shipped"), total: n("118.35") },
];

/** Makes the larder world; the account is the one the doze-aws serves. */
export function seedLarder(endpoint: string): void {
  const cli = runner(endpoint);
  const attrs = (names: [string, string][]) => ["--attribute-definitions", ...names.map(([a, t]) => `AttributeName=${a},AttributeType=${t}`)];
  cli([
    "dynamodb", "create-table", "--table-name", "orders-table", ...attrs([["order_id", "S"], ["created_at", "S"], ["customer_id", "S"]]),
    "--key-schema", "AttributeName=order_id,KeyType=HASH", "AttributeName=created_at,KeyType=RANGE", "--billing-mode", "PAY_PER_REQUEST",
    "--global-secondary-indexes", JSON.stringify([{ IndexName: "by-customer", KeySchema: [{ AttributeName: "customer_id", KeyType: "HASH" }, { AttributeName: "created_at", KeyType: "RANGE" }], Projection: { ProjectionType: "ALL" } }]),
    "--tags", "Key=app,Value=larder", "Key=owner,Value=orders",
  ]);
  cli(["dynamodb", "create-table", "--table-name", "deliveries", ...attrs([["delivery_id", "S"]]), "--key-schema", "AttributeName=delivery_id,KeyType=HASH", "--billing-mode", "PROVISIONED", "--provisioned-throughput", "ReadCapacityUnits=5,WriteCapacityUnits=5"]);
  cli(["dynamodb", "create-table", "--table-name", "search-index", ...attrs([["term", "S"], ["product_id", "S"]]), "--key-schema", "AttributeName=term,KeyType=HASH", "AttributeName=product_id,KeyType=RANGE", "--billing-mode", "PAY_PER_REQUEST"]);
  cli(["dynamodb", "batch-write-item", "--request-items", JSON.stringify({ "orders-table": ORDERS.map((Item) => ({ PutRequest: { Item } })) })]);
  const rows = Array.from({ length: 6 }, (_, i) => ({ PutRequest: { Item: { delivery_id: s(`del-${100 + i}`), carrier: s(i % 2 ? "post" : "van") } } }));
  cli(["dynamodb", "batch-write-item", "--request-items", JSON.stringify({ deliveries: rows })]);
  cli(["s3api", "create-bucket", "--bucket", "larder-exports"]);
  cli(["kinesis", "create-stream", "--stream-name", "orders-changes", "--shard-count", "1"]);
}

if (import.meta.main) {
  const [endpoint] = process.argv.slice(2);
  if (!endpoint) throw new Error("usage: bun dynamodb-seed.ts <endpoint>");
  seedLarder(endpoint);
}
