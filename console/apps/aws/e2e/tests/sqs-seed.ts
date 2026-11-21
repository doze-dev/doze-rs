// The world the SQS boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: seven queues (one FIFO, one
// dead-letter queue with a message, one with tags, a KMS key and an access policy), two SNS topics, a bus with the rule that
// feeds pick-lists, and Lambda functions to poll with. `bun e2e/tests/sqs-seed.ts http://127.0.0.1:44500 [account]` seeds a
// running one by hand.
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

export const queueUrl = (endpoint: string, account: string, name: string) => `${endpoint}/${account}/${name}`;
const arn = (account: string, name: string) => `arn:aws:sqs:us-east-1:${account}:${name}`;

/** The EventBridge statement that lets the rule payment-settled send to pick-lists. */
export const rulePolicy = (account: string) =>
  JSON.stringify({
    Version: "2012-10-17",
    Statement: [
      {
        Sid: "payment-settled",
        Effect: "Allow",
        Principal: { Service: "events.amazonaws.com" },
        Action: "sqs:SendMessage",
        Resource: arn(account, "pick-lists"),
        Condition: { ArnEquals: { "aws:SourceArn": `arn:aws:events:us-east-1:${account}:rule/larder/payment-settled` } },
      },
    ],
  });

/** Makes the larder world; the account is the one the doze-aws serves. */
export function seedLarder(endpoint: string, account: string): void {
  const cli = runner(endpoint);
  const url = (n: string) => queueUrl(endpoint, account, n);
  const key = (JSON.parse(cli(["kms", "create-key"])) as { KeyMetadata: { KeyId: string } }).KeyMetadata.KeyId;
  cli(["kms", "create-alias", "--alias-name", "alias/larder-app", "--target-key-id", key]);

  cli(["sqs", "create-queue", "--queue-name", "pick-lists-dlq", "--attributes", JSON.stringify({ MessageRetentionPeriod: "1209600" })]);
  const redrive = JSON.stringify({ deadLetterTargetArn: arn(account, "pick-lists-dlq"), maxReceiveCount: 5 });
  cli(["sqs", "create-queue", "--queue-name", "pick-lists", "--attributes", JSON.stringify({ RedrivePolicy: redrive, KmsMasterKeyId: "alias/larder-app", KmsDataKeyReusePeriodSeconds: "300", Policy: rulePolicy(account) }), "--tags", "app=fulfilment,env=dev,stack=larder"]);
  for (const n of ["analytics-queue", "inventory-queue", "invoice-queue"]) cli(["sqs", "create-queue", "--queue-name", n]);
  cli(["sqs", "create-queue", "--queue-name", "email-outbox"]);
  cli(["sqs", "create-queue", "--queue-name", "orders-events.fifo", "--attributes", JSON.stringify({ FifoQueue: "true", ContentBasedDeduplication: "true" })]);

  // Messages: pick-lists holds 2 waiting and 1 in flight; the dead-letter queue holds one that was sent straight to it.
  const bodies = [
    JSON.stringify({ pick_list: "PL-4388", order: "ord-20371", lines: 12 }),
    JSON.stringify({ pick_list: "PL-4471", order: "ord-20418", lines: 3 }),
    JSON.stringify({ pick_list: "PL-4472", order: "ord-20419", lines: 7 }),
  ];
  cli(["sqs", "send-message", "--queue-url", url("pick-lists"), "--message-body", bodies[0]!, "--message-attributes", JSON.stringify({ order_channel: { DataType: "String", StringValue: "web" } })]);
  cli(["sqs", "send-message", "--queue-url", url("pick-lists"), "--message-body", bodies[1]!]);
  cli(["sqs", "send-message", "--queue-url", url("pick-lists"), "--message-body", bodies[2]!]);
  cli(["sqs", "receive-message", "--queue-url", url("pick-lists"), "--max-number-of-messages", "1", "--visibility-timeout", "600"]);
  cli(["sqs", "send-message", "--queue-url", url("pick-lists-dlq"), "--message-body", JSON.stringify({ pick_list: "PL-4301", order: "ord-20102", lines: 5 })]);
  for (let i = 0; i < 5; i++) cli(["sqs", "send-message", "--queue-url", url("inventory-queue"), "--message-body", `stock ${i}`]);
  cli(["sqs", "send-message-batch", "--queue-url", url("analytics-queue"), "--entries", JSON.stringify(Array.from({ length: 10 }, (_, i) => ({ Id: `m${i}`, MessageBody: `event ${i}` })))]);

  // SNS: orders-created delivers to three queues; alerts has no subscribers.
  const topic = (n: string) => (JSON.parse(cli(["sns", "create-topic", "--name", n])) as { TopicArn: string }).TopicArn;
  const created = topic("orders-created");
  topic("alerts");
  for (const n of ["invoice-queue", "inventory-queue", "analytics-queue"]) cli(["sns", "subscribe", "--topic-arn", created, "--protocol", "sqs", "--notification-endpoint", arn(account, n)]);

  // EventBridge: the rule that feeds pick-lists.
  cli(["events", "create-event-bus", "--name", "larder"]);
  cli(["events", "put-rule", "--name", "payment-settled", "--event-bus-name", "larder", "--event-pattern", JSON.stringify({ source: ["larder.pay"] })]);
  cli(["events", "put-targets", "--rule", "payment-settled", "--event-bus-name", "larder", "--targets", `Id=1,Arn=${arn(account, "pick-lists")}`]);

  // Lambda: three functions, send-email with a ten-second timeout, the others longer than a 30-second queue allows.
  const dir = mkdtempSync(join(tmpdir(), "sqs-seed-"));
  writeFileSync(join(dir, "index.py"), 'def handler(event, context):\n    return {"ok": True}\n');
  const zip = join(dir, "fn.zip");
  const made = Bun.spawnSync(["zip", "-q", "-j", zip, join(dir, "index.py")]);
  if (made.exitCode !== 0) throw new Error("zip failed");
  cli(["iam", "create-role", "--role-name", "larder-fn", "--assume-role-policy-document", JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] })]);
  for (const [name, timeout] of [["send-email", "10"], ["resize-product-image", "60"], ["export-report", "120"]] as const) {
    cli(["lambda", "create-function", "--function-name", name, "--runtime", "python3.12", "--role", `arn:aws:iam::${account}:role/larder-fn`, "--handler", "index.handler", "--timeout", timeout, "--zip-file", `fileb://${zip}`]);
  }
}

if (import.meta.main) {
  const [endpoint = "http://127.0.0.1:44500", account = "123456789012"] = process.argv.slice(2);
  seedLarder(endpoint, account);
  console.log(`seeded ${endpoint}`);
}
