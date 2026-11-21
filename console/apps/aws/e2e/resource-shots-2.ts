// Screenshots of Resources' second part for a person to look at: a real doze-aws of its own (ports from the environment, else
// 42120 and 42121), resources made with the real AWS CLI (IAM, Logs, Kinesis, CloudWatch, EventBridge, Scheduler, Step
// Functions, API Gateway, CloudFormation, KMS), then their lists, details and dialogs, and Live with the console's own calls
// hidden, light and dark, in each browser.
//   bun e2e/resource-shots-2.ts <out dir> [firefox,webkit,chromium]
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws, start } from "./server";

const [out = "resource-shots-2", only = "firefox,webkit,chromium"] = process.argv.slice(2);
const listen = Number(process.env.DOZE_AWS_SHOTS_PORT ?? 42120);
const server = await start(listen, listen + 1);
const engines = { firefox, webkit, chromium };

function run(args: string[]) {
  const r = aws(server.endpoint, args, { accessKey: "AKIALARDERAPP0000001" });
  if (r.code !== 0) console.warn(`aws ${args.join(" ")}: ${r.out}`);
  return r;
}

const enc = encodeURIComponent;

try {
  const acct = server.account;
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "states.amazonaws.com" }, Action: "sts:AssumeRole" }] });
  run(["iam", "create-role", "--role-name", "checkout-flow", "--assume-role-policy-document", trust, "--description", "Runs the checkout state machine"]);
  run(["iam", "create-role", "--role-name", "orders-handler", "--assume-role-policy-document", trust.replace("states", "lambda")]);
  const doc = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: ["sqs:SendMessage"], Resource: "*" }] });
  run(["iam", "create-policy", "--policy-name", "orders-send", "--policy-document", doc, "--description", "Send to the order queues"]);
  run(["iam", "attach-role-policy", "--role-name", "orders-handler", "--policy-arn", `arn:aws:iam::${acct}:policy/orders-send`]);
  run(["iam", "create-user", "--user-name", "ci-deployer"]);
  run(["iam", "create-access-key", "--user-name", "ci-deployer"]);
  run(["iam", "create-group", "--group-name", "developers"]);
  run(["iam", "add-user-to-group", "--user-name", "ci-deployer", "--group-name", "developers"]);
  run(["logs", "create-log-group", "--log-group-name", "/aws/lambda/orders-handler"]);
  run(["logs", "put-retention-policy", "--log-group-name", "/aws/lambda/orders-handler", "--retention-in-days", "14"]);
  for (const s of ["2026/11/21/[$LATEST]a1b2", "2026/11/21/[$LATEST]c3d4"]) {
    run(["logs", "create-log-stream", "--log-group-name", "/aws/lambda/orders-handler", "--log-stream-name", s]);
    run(["logs", "put-log-events", "--log-group-name", "/aws/lambda/orders-handler", "--log-stream-name", s, "--log-events", `timestamp=${Date.now()},message=START`]);
  }
  run(["kinesis", "create-stream", "--stream-name", "clickstream", "--shard-count", "2"]);
  run(["cloudwatch", "put-metric-data", "--namespace", "Larder", "--metric-name", "OrdersPlaced", "--value", "3", "--dimensions", "env=dev"]);
  run(["cloudwatch", "put-metric-alarm", "--alarm-name", "orders-backlog", "--metric-name", "ApproximateNumberOfMessagesVisible", "--namespace", "AWS/SQS", "--statistic", "Maximum", "--period", "60", "--evaluation-periods", "2", "--threshold", "100", "--comparison-operator", "GreaterThanThreshold", "--dimensions", "Name=QueueName,Value=orders"]);
  run(["sqs", "create-queue", "--queue-name", "orders"]);
  run(["events", "create-event-bus", "--name", "orders"]);
  run(["events", "put-rule", "--name", "on-order-placed", "--event-bus-name", "orders", "--event-pattern", '{"source":["larder.orders"]}']);
  run(["events", "put-targets", "--rule", "on-order-placed", "--event-bus-name", "orders", "--targets", `Id=queue,Arn=arn:aws:sqs:us-east-1:${acct}:orders`]);
  run(["events", "put-rule", "--name", "nightly-cleanup", "--schedule-expression", "rate(1 day)"]);
  run(["scheduler", "create-schedule", "--name", "nightly-report", "--schedule-expression", "cron(0 2 * * ? *)", "--flexible-time-window", "Mode=OFF", "--target", `Arn=arn:aws:sqs:us-east-1:${acct}:orders,RoleArn=arn:aws:iam::${acct}:role/checkout-flow`]);
  const machine = JSON.stringify({ StartAt: "Reserve", States: { Reserve: { Type: "Pass", Next: "Charge" }, Charge: { Type: "Pass", End: true } } });
  const sm = run(["stepfunctions", "create-state-machine", "--name", "checkout", "--definition", machine, "--role-arn", `arn:aws:iam::${acct}:role/checkout-flow`]);
  const smArn = sm.code === 0 ? (JSON.parse(sm.out) as { stateMachineArn: string }).stateMachineArn : "";
  for (const n of ["order-1041", "order-1042", "order-1043"]) run(["stepfunctions", "start-execution", "--state-machine-arn", smArn, "--name", n, "--input", '{"order":1}']);
  run(["apigateway", "create-rest-api", "--name", "orders-api", "--description", "Orders, REST"]);
  run(["apigatewayv2", "create-api", "--name", "orders-http", "--protocol-type", "HTTP"]);
  const template = join(mkdirSafe(), "stack.json");
  writeFileSync(template, JSON.stringify({ Description: "Larder's order pipeline", Resources: { Inbox: { Type: "AWS::SQS::Queue", Properties: { QueueName: "stack-inbox" } }, Events: { Type: "AWS::SNS::Topic", Properties: { TopicName: "stack-events" } } }, Outputs: { InboxUrl: { Value: { Ref: "Inbox" }, Description: "Where orders arrive" }, EventsArn: { Value: { Ref: "Events" }, Export: { Name: "larder-events" } } } }));
  run(["cloudformation", "create-stack", "--stack-name", "larder-pipeline", "--template-body", `file://${template}`]);
  run(["sns", "create-topic", "--name", "order-placed"]);
  run(["s3api", "create-bucket", "--bucket", "product-images"]);
  run(["kms", "create-key", "--description", "larder app key"]);
  run(["kms", "create-alias", "--alias-name", "alias/larder", "--target-key-id", JSON.parse(run(["kms", "list-keys"]).out).Keys[0].KeyId]);
  // A secret with no key of its own: Secrets Manager makes the AWS managed aws/secretsmanager key.
  run(["secretsmanager", "create-secret", "--name", "larder/db", "--secret-string", "x"]);
  await new Promise((r) => setTimeout(r, 800));

  async function settle(p: Page, title: string) {
    await p.locator("h1", { hasText: title }).first().waitFor();
    await p.evaluate(() => document.fonts.ready);
    await p.waitForTimeout(500);
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
      await page("/resources", "Resources", "index");
      await page("/resources/iam/roles", "Roles", "iam-roles", false);
      await p.getByTestId("create-open").click();
      await p.getByTestId("create-dialog").waitFor();
      await p.waitForTimeout(400);
      await shot("iam-role-create");
      await p.keyboard.press("Escape");
      await page("/resources/iam/roles/orders-handler", "orders-handler", "iam-role");
      await page("/resources/iam/users/ci-deployer", "ci-deployer", "iam-user");
      await page(`/resources/iam/policies/${enc(`arn:aws:iam::${acct}:policy/orders-send`)}`, "orders-send", "iam-policy");
      await page(`/resources/logs/groups/${enc("/aws/lambda/orders-handler")}`, "/aws/lambda/orders-handler", "log-group");
      await page("/resources/kinesis/streams/clickstream", "clickstream", "stream");
      await page("/resources/cloudwatch/alarms/orders-backlog", "orders-backlog", "alarm");
      await page("/resources/cloudwatch/metrics", "Metrics", "metrics", false);
      await page(`/resources/eventbridge/rules/${enc("orders/on-order-placed")}`, "orders/on-order-placed", "rule");
      await page("/resources/eventbridge/buses/orders", "orders", "bus");
      await page(`/resources/scheduler/schedules/${enc("default/nightly-report")}`, "default/nightly-report", "schedule");
      await page("/resources/stepfunctions/machines/checkout", "checkout", "machine");
      await page("/resources/stepfunctions/machines", "State machines", "machines", false);
      await page("/resources/apigateway/apis", "HTTP and WebSocket APIs", "http-apis", false);
      await page("/resources/cloudformation/stacks/larder-pipeline", "larder-pipeline", "stack");
      await page("/resources/kms/keys", "Keys", "kms-keys", false);
      await page("/resources/kms/keys?all=yes", "Keys", "kms-keys-all", false);
      await page("/resources/s3/buckets", "Buckets", "buckets", false);
      await page("/resources/sns/topics/order-placed", "order-placed", "topic", false);
      await p.getByTestId("sub-add").click();
      await p.getByTestId("create-dialog").waitFor();
      await p.waitForTimeout(400);
      await shot("topic-add-subscription");
      await p.keyboard.press("Escape");
      await page("/live", "Live", "live", false);
      await ctx.close();
    }
    await b.close();
  }
} finally {
  await server.stop();
}

function mkdirSafe(): string {
  const dir = join(tmpdir(), `doze-aws-shots-${process.pid}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}
