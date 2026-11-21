// The world the CloudWatch Logs and Kinesis boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: log groups with their
// retention, streams, events and metric filters, a Lambda function that is a subscription's destination, and Kinesis streams with tags, a consumer and
// records. `bun e2e/tests/logskinesis-seed.ts http://127.0.0.1:45030 [account]` seeds a running one by hand.
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aws } from "../server";

type Cli = (args: string[]) => string;

function runner(endpoint: string): Cli {
  return (args) => {
    const r = aws(endpoint, args);
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
}

/** The streams of /larder/api and what each wrote lately (minutes back, the message). */
const API: Record<string, [number, string][]> = {
  "payments-8c2e1": [
    [8, '{"level":"info","msg":"payment settled","order_id":"ord_91c7"}'],
    [7, '{"level":"warn","msg":"stripe retry","attempt":2,"order_id":"ord_91c9"}'],
    [6, '{"level":"error","app":"payments","msg":"stripe timeout","order_id":"ord_91c9","amount":18.4,"elapsed_ms":10004}'],
    [5, '{"level":"info","msg":"payment settled","order_id":"ord_91c9"}'],
    [4, '{"level":"info","msg":"refund requested","order_id":"ord_90f2"}'],
  ],
  "orders-5d6f9": [[9, '{"level":"info","msg":"order placed","order_id":"ord_91c9"}'], [3, '{"level":"info","msg":"order placed","order_id":"ord_91ca"}']],
  "catalog-1b7a4": [[12, '{"level":"info","msg":"catalog refreshed","items":412}']],
  "fulfilment-3e90c": [[15, '{"level":"error","msg":"pick list write failed","order_id":"ord_9102"}']],
  "search-indexer-6a2d7": [[40, '{"level":"info","msg":"index compacted"}']],
};

/** Makes the larder world. */
export function seedLarder(endpoint: string, account: string): void {
  const cli = runner(endpoint);
  const now = Date.now();
  // The function a subscription filter sends to.
  const dir = mkdtempSync(join(tmpdir(), "lk-seed-"));
  writeFileSync(join(dir, "index.py"), 'def handler(event, context):\n    return {"ok": True}\n');
  const zip = join(dir, "fn.zip");
  if (Bun.spawnSync(["zip", "-q", "-j", zip, join(dir, "index.py")]).exitCode !== 0) throw new Error("zip failed");
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });
  cli(["iam", "create-role", "--role-name", "larder-fn", "--assume-role-policy-document", trust]);
  for (const fn of ["send-email", "resize-product-image"]) {
    cli(["lambda", "create-function", "--function-name", fn, "--runtime", "python3.13", "--role", `arn:aws:iam::${account}:role/larder-fn`, "--handler", "index.handler", "--timeout", "10", "--zip-file", `fileb://${zip}`]);
  }

  for (const g of ["/aws/lambda/export-report", "/aws/lambda/resize-product-image", "/aws/lambda/send-email", "/larder/api", "/larder/scratch", "/larder/worker"]) cli(["logs", "create-log-group", "--log-group-name", g]);
  cli(["logs", "put-retention-policy", "--log-group-name", "/aws/lambda/resize-product-image", "--retention-in-days", "14"]);
  cli(["logs", "put-retention-policy", "--log-group-name", "/aws/lambda/send-email", "--retention-in-days", "14"]);
  cli(["logs", "put-retention-policy", "--log-group-name", "/larder/api", "--retention-in-days", "30"]);
  for (const [stream, events] of Object.entries(API)) {
    cli(["logs", "create-log-stream", "--log-group-name", "/larder/api", "--log-stream-name", stream]);
    cli(["logs", "put-log-events", "--log-group-name", "/larder/api", "--log-stream-name", stream, "--log-events", JSON.stringify(events.map(([m, message]) => ({ timestamp: now - m * 60_000, message })))]);
  }
  const filter = (group: string, name: string, pattern: string, ns: string, metric: string) =>
    cli(["logs", "put-metric-filter", "--log-group-name", group, "--filter-name", name, "--filter-pattern", pattern, "--metric-transformations", `metricName=${metric},metricNamespace=${ns},metricValue=1,unit=Count`]);
  filter("/larder/api", "api-errors", '{ $.level = "error" }', "Larder/Api", "ApiErrors");
  filter("/larder/api", "orders-placed", '{ $.msg = "order placed" }', "Larder/Orders", "OrdersPlaced");
  filter("/aws/lambda/resize-product-image", "resize-errors", "ERROR", "Larder/Images", "ResizeErrors");

  // Kinesis: clickstream (provisioned, 2 shards, 3 tags, a consumer, records), orders-feed (on demand, no tags), inventory-feed (provisioned).
  cli(["kinesis", "create-stream", "--stream-name", "clickstream", "--shard-count", "2"]);
  cli(["kinesis", "wait", "stream-exists", "--stream-name", "clickstream"]);
  cli(["kinesis", "add-tags-to-stream", "--stream-name", "clickstream", "--tags", "stack=larder,env=dev,owner=catalog"]);
  const arn = (JSON.parse(cli(["kinesis", "describe-stream-summary", "--stream-name", "clickstream"])) as { StreamDescriptionSummary: { StreamARN: string } }).StreamDescriptionSummary.StreamARN;
  cli(["kinesis", "register-stream-consumer", "--stream-arn", arn, "--consumer-name", "search-indexer"]);
  const events = ['{"event":"page_view","sku":"FV-TOM-V","session":"s-0a3d"}', '{"event":"add_to_cart","sku":"DA-MLK-1L","qty":2,"session":"s-0a3d"}', '{"event":"search","q":"tomatoes","session":"s-91b7"}', '{"event":"page_view","sku":"BK-SOU-500","session":"s-91b7"}', '{"event":"checkout_start","cart":"c-20371","session":"s-0a3d"}', '{"event":"page_view","sku":"FV-TOM-V","session":"s-33f0"}'];
  const keys = ["s-0a3d", "s-0a3d", "s-91b7", "s-91b7", "s-0a3d", "s-33f0"];
  events.forEach((data, i) => cli(["kinesis", "put-record", "--stream-name", "clickstream", "--partition-key", keys[i] ?? "s", "--data", Buffer.from(data).toString("base64")]));
  cli(["kinesis", "create-stream", "--stream-name", "orders-feed", "--stream-mode-details", "StreamMode=ON_DEMAND"]);
  cli(["kinesis", "create-stream", "--stream-name", "inventory-feed", "--shard-count", "2"]);
  cli(["iam", "create-role", "--role-name", "report-job", "--assume-role-policy-document", JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { AWS: `arn:aws:iam::${account}:root` }, Action: "sts:AssumeRole" }] })]);
  const key = (JSON.parse(cli(["kms", "create-key", "--description", "Larder application data key"])) as { KeyMetadata: { KeyId: string } }).KeyMetadata.KeyId;
  cli(["kms", "create-alias", "--alias-name", "alias/larder-app", "--target-key-id", key]);
}

if (import.meta.main) {
  const [endpoint = "http://127.0.0.1:45030", account = "111122223333"] = process.argv.slice(2);
  seedLarder(endpoint, account);
  console.log(`seeded ${endpoint}`);
}
