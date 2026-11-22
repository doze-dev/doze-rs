// The world the Home, Connect, Live and direction boards are drawn in ("larder"): the apps are IAM users with an access key of
// their own (payments, fulfilment, catalog, report-job, admin-cli), each calling as its own SDK (Go, Python, the AWS CLI), so a
// row says who made it. `bun e2e/tests/home-seed.ts http://127.0.0.1:45860 [account]` seeds a running doze-aws by hand.
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aws } from "../server";

export const GO = "aws-sdk-go-v2/1.26.0 ua/2.0 os/linux lang/go#1.22.1 md/GOOS#linux";
export const PY = "Boto3/1.34.69 md/Botocore#1.34.69 ua/2.0 os/linux lang/python#3.12.2";

export type Apps = { payments: string; fulfilment: string; catalog: string; reportJob: string; adminCli: string };
export const APP_NAMES: Record<keyof Apps, string> = { payments: "payments", fulfilment: "fulfilment", catalog: "catalog", reportJob: "report-job", adminCli: "admin-cli" };

const arn = (service: string, account: string, name: string) => `arn:aws:${service}:us-east-1:${account}:${name}`;
const stamp = () => new Date().toISOString().replace(/[-:]|\.\d+/g, "");

/** One request as an SDK sends it (a JSON protocol call, signed for `scope` with `key`; doze-aws trusts the signature), with the
 * User-Agent of the client it stands for. Returns the status. */
export async function rawJson(endpoint: string, key: string, scope: string, target: string, body: unknown, ua: string, version = "1.0"): Promise<number> {
  const date = stamp();
  const r = await fetch(`${endpoint}/`, {
    method: "POST",
    headers: { "Content-Type": `application/x-amz-json-${version}`, "X-Amz-Target": target, "X-Amz-Date": date, "User-Agent": ua, Authorization: `AWS4-HMAC-SHA256 Credential=${key}/${date.slice(0, 8)}/us-east-1/${scope}/aws4_request, SignedHeaders=host, Signature=00` },
    body: JSON.stringify(body),
  });
  await r.arrayBuffer();
  return r.status;
}

/** A PutObject as an SDK sends it. */
export async function rawPut(endpoint: string, key: string, path: string, body: string, ua: string): Promise<number> {
  const date = stamp();
  const r = await fetch(`${endpoint}/${path}`, { method: "PUT", headers: { "X-Amz-Date": date, "User-Agent": ua, "X-Amz-Content-Sha256": "UNSIGNED-PAYLOAD", Authorization: `AWS4-HMAC-SHA256 Credential=${key}/${date.slice(0, 8)}/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=00` }, body });
  await r.arrayBuffer();
  return r.status;
}

/** A call with the AWS CLI as the app whose key it is. */
export function as(endpoint: string, key: string, args: string[]): { code: number; out: string } {
  return aws(endpoint, args, { accessKey: key });
}

/** Makes the world; the account is the one the doze-aws serves. */
export function seedHome(endpoint: string, account: string): Apps {
  const cli = (args: string[]) => {
    const r = aws(endpoint, args);
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
  const allow = (actions: string[], resources: string[]) => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: actions, Resource: resources }] });
  const keys = {} as Apps;
  for (const [id, name] of Object.entries(APP_NAMES) as [keyof Apps, string][]) {
    cli(["iam", "create-user", "--user-name", name]);
    keys[id] = (JSON.parse(cli(["iam", "create-access-key", "--user-name", name])) as { AccessKey: { AccessKeyId: string } }).AccessKey.AccessKeyId;
    // The report job may read only what is under reports/; every other app may do anything.
    const policy = id === "reportJob" ? allow(["s3:GetObject", "s3:PutObject", "s3:ListBucket"], ["arn:aws:s3:::reports/*", "arn:aws:s3:::reports"]) : allow(["*"], ["*"]);
    cli(["iam", "put-user-policy", "--user-name", name, "--policy-name", id === "reportJob" ? "report-job-readonly" : "app", "--policy-document", policy]);
  }

  // SQS: pick-lists (fed by a rule, dead letters to pick-lists-dlq after 5 receives), and others.
  cli(["sqs", "create-queue", "--queue-name", "pick-lists-dlq", "--attributes", JSON.stringify({ MessageRetentionPeriod: "1209600" })]);
  const redrive = JSON.stringify({ deadLetterTargetArn: arn("sqs", account, "pick-lists-dlq"), maxReceiveCount: 5 });
  cli(["sqs", "create-queue", "--queue-name", "pick-lists", "--attributes", JSON.stringify({ RedrivePolicy: redrive })]);
  for (const n of ["analytics-queue", "inventory-queue", "invoice-queue", "email-outbox"]) cli(["sqs", "create-queue", "--queue-name", n]);
  const url = (n: string) => `${endpoint}/${account}/${n}`;
  for (const i of [1, 2, 3]) cli(["sqs", "send-message", "--queue-url", url("pick-lists"), "--message-body", JSON.stringify({ pick_list: `PL-${4300 + i}` })]);
  cli(["sqs", "receive-message", "--queue-url", url("pick-lists"), "--max-number-of-messages", "1", "--visibility-timeout", "600"]);
  cli(["sqs", "send-message", "--queue-url", url("pick-lists-dlq"), "--message-body", JSON.stringify({ pick_list: "PL-4301" })]);

  // EventBridge: the rule that feeds pick-lists.
  cli(["events", "create-event-bus", "--name", "larder"]);
  cli(["events", "put-rule", "--name", "payment-settled", "--event-bus-name", "larder", "--event-pattern", JSON.stringify({ source: ["larder.pay"] })]);
  cli(["events", "put-targets", "--rule", "payment-settled", "--event-bus-name", "larder", "--targets", `Id=1,Arn=${arn("sqs", account, "pick-lists")}`]);

  // SNS: orders-created delivers to two queues; alerts has an email subscription nothing is delivered to.
  const topic = (n: string) => (JSON.parse(cli(["sns", "create-topic", "--name", n])) as { TopicArn: string }).TopicArn;
  const created = topic("orders-created");
  const alerts = topic("alerts");
  for (const n of ["invoice-queue", "inventory-queue"]) cli(["sns", "subscribe", "--topic-arn", created, "--protocol", "sqs", "--notification-endpoint", arn("sqs", account, n)]);
  cli(["sns", "subscribe", "--topic-arn", alerts, "--protocol", "email", "--notification-endpoint", "alerts@larder.dev"]);

  // S3 and Lambda: product-images notifies resize-product-image when an object is created; a reports bucket; receipts.
  const dir = mkdtempSync(join(tmpdir(), "home-seed-"));
  writeFileSync(join(dir, "index.py"), 'def handler(event, context):\n    return {"ok": True}\n');
  const zip = join(dir, "fn.zip");
  if (Bun.spawnSync(["zip", "-q", "-j", zip, join(dir, "index.py")]).exitCode !== 0) throw new Error("zip failed");
  cli(["iam", "create-role", "--role-name", "larder-fn", "--assume-role-policy-document", JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] })]);
  for (const name of ["resize-product-image", "send-email"]) cli(["lambda", "create-function", "--function-name", name, "--runtime", "python3.12", "--role", `arn:aws:iam::${account}:role/larder-fn`, "--handler", "index.handler", "--zip-file", `fileb://${zip}`]);
  for (const b of ["product-images", "reports", "receipts"]) cli(["s3api", "create-bucket", "--bucket", b]);
  writeFileSync(join(dir, "r.txt"), "receipt");
  cli(["s3api", "put-object", "--bucket", "receipts", "--key", "r-8841.pdf", "--body", join(dir, "r.txt")]);
  cli(["s3api", "put-object", "--bucket", "reports", "--key", "2026-11.csv", "--body", join(dir, "r.txt")]);
  cli(["lambda", "add-permission", "--function-name", "resize-product-image", "--statement-id", "s3", "--action", "lambda:InvokeFunction", "--principal", "s3.amazonaws.com", "--source-arn", "arn:aws:s3:::product-images"]);
  cli(["s3api", "put-bucket-notification-configuration", "--bucket", "product-images", "--notification-configuration", JSON.stringify({ LambdaFunctionConfigurations: [{ LambdaFunctionArn: arn("lambda", account, "function:resize-product-image"), Events: ["s3:ObjectCreated:*"] }] })]);

  // DynamoDB: the orders table; a table nothing calls.
  cli(["dynamodb", "create-table", "--table-name", "orders-table", "--attribute-definitions", "AttributeName=id,AttributeType=S", "--key-schema", "AttributeName=id,KeyType=HASH", "--billing-mode", "PAY_PER_REQUEST"]);
  cli(["dynamodb", "create-table", "--table-name", "deliveries", "--attribute-definitions", "AttributeName=id,AttributeType=S", "--key-schema", "AttributeName=id,KeyType=HASH", "--billing-mode", "PAY_PER_REQUEST"]);
  return keys;
}

/** What the apps do at the edge of the boards: a refusal, a call that would be denied, a call not built, and ordinary traffic. */
export async function trafficHome(endpoint: string, account: string, k: Apps): Promise<void> {
  const url = (n: string) => `${endpoint}/${account}/${n}`;
  // fulfilment (Python) polls pick-lists, then asks for a queue name SQS refuses.
  await rawJson(endpoint, k.fulfilment, "sqs", "AmazonSQS.ReceiveMessage", { QueueUrl: url("pick-lists"), MaxNumberOfMessages: 1 }, PY);
  await rawJson(endpoint, k.fulfilment, "sqs", "AmazonSQS.DeleteMessage", { QueueUrl: url("pick-lists"), ReceiptHandle: "x" }, PY);
  await rawJson(endpoint, k.fulfilment, "sqs", "AmazonSQS.CreateQueue", { QueueName: "pick lists!" }, PY);
  // payments (Go) puts events and sends to pick-lists; catalog (Go) writes an item and an object.
  await rawJson(endpoint, k.payments, "events", "AWSEvents.PutEvents", { Entries: [{ EventBusName: "larder", Source: "larder.pay", DetailType: "settled", Detail: "{}" }] }, GO, "1.1");
  await rawJson(endpoint, k.payments, "sqs", "AmazonSQS.SendMessage", { QueueUrl: url("pick-lists"), MessageBody: "{}" }, GO);
  await rawJson(endpoint, k.catalog, "dynamodb", "DynamoDB_20120810.PutItem", { TableName: "orders-table", Item: { id: { S: "o-1" } } }, GO, "1.0");
  await rawPut(endpoint, k.catalog, "product-images/sku-FV-TOM-V.jpg", "jpg", GO);
  // report-job reads a bucket its policy does not cover: soft IAM lets it through and records that AWS would not.
  as(endpoint, k.reportJob, ["s3api", "get-object", "--bucket", "receipts", "--key", "r-8841.pdf", join(tmpdir(), "home-seed-out")]);
  // admin-cli asks for S3 Select, which doze-aws does not build: 501.
  as(endpoint, k.adminCli, ["s3api", "select-object-content", "--bucket", "reports", "--key", "2026-11.csv", "--expression", "select * from s3object", "--expression-type", "SQL", "--input-serialization", '{"CSV":{}}', "--output-serialization", '{"CSV":{}}', join(tmpdir(), "home-seed-sel")]);
}

if (import.meta.main) {
  // `setup` makes the world and keeps the apps' keys in a file; `traffic` has the apps call (after doze-aws started again, so the
  // Live record holds only that); no mode does both.
  const [endpoint = "http://127.0.0.1:45860", mode = "both", file = join(tmpdir(), "home-seed-keys.json")] = process.argv.slice(2);
  const account = "123456789012";
  let keys: Apps;
  if (mode === "traffic") keys = JSON.parse(readFileSync(file, "utf8")) as Apps;
  else {
    keys = seedHome(endpoint, account);
    writeFileSync(file, JSON.stringify(keys));
  }
  if (mode !== "setup") await trafficHome(endpoint, account, keys);
  console.log(`${mode}: ${endpoint}`);
}
