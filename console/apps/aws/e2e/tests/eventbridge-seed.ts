// The world the EventBridge boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: the queues and the topic the
// rules deliver to (with the policies that let EventBridge publish), a KMS alias, the bus larder (tagged) with three archives, and the rules
// order-placed (an SNS target with an input transformer, a retry policy and a dead-letter queue), payment-settled (an SQS target), audit-sweep
// (no target) and, on the default bus, nightly-export (a cron). The bus larder-audit, the connections, the API destinations and the replays are
// made by the boards' own flows.
// `bun e2e/tests/eventbridge-seed.ts http://127.0.0.1:45400 [account]` seeds a running one by hand.
import { aws } from "../server";

type Cli = (args: string[]) => string;

function runner(endpoint: string): Cli {
  return (args) => {
    const r = aws(endpoint, args);
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
}

export const queueArn = (account: string, name: string) => `arn:aws:sqs:us-east-1:${account}:${name}`;
export const topicArn = (account: string, name: string) => `arn:aws:sns:us-east-1:${account}:${name}`;
export const busArn = (account: string, name: string) => `arn:aws:events:us-east-1:${account}:event-bus/${name}`;

export const PATTERN = { source: ["larder.orders"], "detail-type": ["OrderPlaced"], detail: { total_cents: [{ numeric: [">", 0] }] } };
export const PAYMENT_PATTERN = { source: ["larder.payments"], "detail-type": ["PaymentSettled"] };

/** Makes the larder world; the account is the one the doze-aws serves. */
export function seedLarder(endpoint: string, account: string): void {
  const cli = runner(endpoint);
  const key = (JSON.parse(cli(["kms", "create-key", "--description", "Larder application data key"])) as { KeyMetadata: { KeyId: string } }).KeyMetadata.KeyId;
  cli(["kms", "create-alias", "--alias-name", "alias/larder-app", "--target-key-id", key]);
  for (const n of ["invoice-queue", "inventory-queue", "analytics-queue", "pick-lists", "pick-lists-dlq"]) cli(["sqs", "create-queue", "--queue-name", n]);
  const topic = (JSON.parse(cli(["sns", "create-topic", "--name", "orders-created"])) as { TopicArn: string }).TopicArn;
  for (const n of ["invoice-queue", "inventory-queue", "analytics-queue"]) cli(["sns", "subscribe", "--topic-arn", topic, "--protocol", "sqs", "--notification-endpoint", queueArn(account, n)]);
  // EventBridge delivers as events.amazonaws.com, for the rule: the topic's and the queue's own policies say it may.
  const allow = (action: string, resource: string) => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "events.amazonaws.com" }, Action: action, Resource: resource }] });
  cli(["sns", "set-topic-attributes", "--topic-arn", topic, "--attribute-name", "Policy", "--attribute-value", allow("sns:Publish", topic)]);
  const url = (JSON.parse(cli(["sqs", "get-queue-url", "--queue-name", "pick-lists"])) as { QueueUrl: string }).QueueUrl;
  cli(["sqs", "set-queue-attributes", "--queue-url", url, "--attributes", JSON.stringify({ Policy: allow("sqs:SendMessage", queueArn(account, "pick-lists")) })]);

  cli(["events", "create-event-bus", "--name", "larder", "--description", "Application events for the larder stack", "--tags", "Key=Environment,Value=dev", "Key=Team,Value=platform"]);
  cli(["events", "create-archive", "--archive-name", "larder-all", "--event-source-arn", busArn(account, "larder"), "--retention-days", "30"]);
  cli(["events", "create-archive", "--archive-name", "orders-30d", "--event-source-arn", busArn(account, "larder"), "--retention-days", "30", "--event-pattern", JSON.stringify({ source: ["larder.orders"] })]);
  cli(["events", "create-archive", "--archive-name", "payments-90d", "--event-source-arn", busArn(account, "larder"), "--retention-days", "90", "--event-pattern", JSON.stringify({ source: ["larder.payments"] })]);

  cli(["events", "put-rule", "--name", "order-placed", "--event-bus-name", "larder", "--event-pattern", JSON.stringify(PATTERN), "--description", "Fan each new order out to its queues", "--tags", "Key=Environment,Value=dev", "Key=Team,Value=checkout"]);
  const target = {
    Id: "publish-orders-created",
    Arn: topic,
    InputTransformer: { InputPathsMap: { order_id: "$.detail.order_id", customer: "$.detail.customer_id" }, InputTemplate: '{"order_id": <order_id>, "customer": <customer>}' },
    RetryPolicy: { MaximumRetryAttempts: 20, MaximumEventAgeInSeconds: 3600 },
    DeadLetterConfig: { Arn: queueArn(account, "pick-lists-dlq") },
  };
  cli(["events", "put-targets", "--rule", "order-placed", "--event-bus-name", "larder", "--targets", JSON.stringify([target])]);
  cli(["events", "put-rule", "--name", "payment-settled", "--event-bus-name", "larder", "--event-pattern", JSON.stringify(PAYMENT_PATTERN), "--description", "Queue a pick list once a payment settles"]);
  cli(["events", "put-targets", "--rule", "payment-settled", "--event-bus-name", "larder", "--targets", JSON.stringify([{ Id: "pick-lists", Arn: queueArn(account, "pick-lists") }])]);
  cli(["events", "put-rule", "--name", "audit-sweep", "--event-bus-name", "larder", "--event-pattern", JSON.stringify({ source: ["larder.audit"] }), "--description", "Sweep what the audit trail should keep"]);
  cli(["events", "put-rule", "--name", "nightly-export", "--schedule-expression", "cron(0 2 * * ? *)", "--description", "Export the day's orders every night"]);
}

/** One event put on `bus`, as an app would. */
export function putEvent(endpoint: string, bus: string, source: string, detailType: string, detail: object): { code: number; out: string } {
  return aws(endpoint, ["events", "put-events", "--entries", JSON.stringify([{ EventBusName: bus, Source: source, DetailType: detailType, Detail: JSON.stringify(detail) }])]);
}

if (import.meta.main) {
  const [endpoint = "http://127.0.0.1:45400", account = "111122223333"] = process.argv.slice(2);
  seedLarder(endpoint, account);
  putEvent(endpoint, "larder", "larder.orders", "OrderPlaced", { order_id: "o-20931", customer_id: "c-1187", total_cents: 4850 });
  console.log("seeded", endpoint);
}
