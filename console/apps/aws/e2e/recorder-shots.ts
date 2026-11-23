// Screenshots of the result summaries for a person to look at: a real doze-aws of its own (ports from the environment, else 42041
// and 42042), traffic made with the real AWS CLI (an SQS send and receive, an SNS publish that delivers to two queues, S3 objects
// and a listing, DynamoDB, Kinesis, EventBridge, Logs, an AssumeRole), then the wire, the drawer of a call, and the Recent calls of
// a queue, a topic and an object, light and dark.
//   bun e2e/recorder-shots.ts <out dir> [firefox,webkit,chromium]
import { mkdirSync } from "node:fs";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws, start } from "./server";

const [out = "recorder-shots", only = "chromium"] = process.argv.slice(2);
const listen = Number(process.env.DOZE_AWS_SHOTS_PORT ?? 42041);
const server = await start(listen, listen + 1);
const engines = { firefox, webkit, chromium };

function run(args: string[], key = "AKIAORDERSAPP0000001") {
  return aws(server.endpoint, args, { accessKey: key });
}

try {
  const acct = server.account;
  const url = `${server.endpoint}/${acct}/pick-lists`;
  run(["sqs", "create-queue", "--queue-name", "pick-lists"]);
  run(["sqs", "create-queue", "--queue-name", "audit"]);
  run(["sns", "create-topic", "--name", "order-placed"]);
  const topic = `arn:aws:sns:us-east-1:${acct}:order-placed`;
  for (const q of ["pick-lists", "audit"]) run(["sns", "subscribe", "--topic-arn", topic, "--protocol", "sqs", "--notification-endpoint", `arn:aws:sqs:us-east-1:${acct}:${q}`]);
  run(["sns", "subscribe", "--topic-arn", topic, "--protocol", "email", "--notification-endpoint", "ops@example.test"]);
  for (let i = 0; i < 2; i++) run(["sns", "publish", "--topic-arn", topic, "--message", `order ${i}`]);
  run(["sqs", "send-message", "--queue-url", url, "--message-body", "pick the red tomatoes from row 4"], "AKIAPICKERS00000001");
  run(["sqs", "receive-message", "--queue-url", url, "--max-number-of-messages", "10"], "AKIAPICKERS00000001");
  run(["sqs", "purge-queue", "--queue-url", url]);
  run(["sqs", "receive-message", "--queue-url", url], "AKIAPICKERS00000001");
  run(["s3api", "create-bucket", "--bucket", "product-images"]);
  for (const k of ["sku/tomato.jpg", "sku/radish.jpg", "hero.jpg"]) run(["s3api", "put-object", "--bucket", "product-images", "--key", k, "--body", import.meta.filename], "AKIACATALOG0000001");
  run(["s3api", "list-objects-v2", "--bucket", "product-images"], "AKIACATALOG0000001");
  run(["s3api", "get-object", "--bucket", "product-images", "--key", "sku/tomato.jpg", "/dev/null"], "AKIACATALOG0000001");
  run(["dynamodb", "create-table", "--table-name", "orders", "--attribute-definitions", "AttributeName=id,AttributeType=S", "--key-schema", "AttributeName=id,KeyType=HASH", "--billing-mode", "PAY_PER_REQUEST"]);
  run(["dynamodb", "put-item", "--table-name", "orders", "--item", '{"id":{"S":"o-1"}}']);
  run(["dynamodb", "scan", "--table-name", "orders"]);
  run(["kinesis", "create-stream", "--stream-name", "clicks", "--shard-count", "1"]);
  run(["kinesis", "put-record", "--stream-name", "clicks", "--data", "aGk=", "--partition-key", "p"]);
  run(["events", "put-events", "--entries", JSON.stringify([{ Source: "larder.payments", DetailType: "settled", Detail: "{}" }, { Source: "larder.payments", DetailType: "settled", Detail: "{}" }])], "AKIAPAYMENTS00000001");
  run(["logs", "create-log-group", "--log-group-name", "api"]);
  run(["logs", "create-log-stream", "--log-group-name", "api", "--log-stream-name", "s1"]);
  run(["logs", "put-log-events", "--log-group-name", "api", "--log-stream-name", "s1", "--log-events", ...[1, 2, 3].map((n) => `timestamp=${Date.now()},message=line ${n}`)]);
  run(["iam", "create-role", "--role-name", "report-job", "--assume-role-policy-document", '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"*"},"Action":"sts:AssumeRole"}]}']);
  run(["sts", "assume-role", "--role-arn", `arn:aws:iam::${acct}:role/report-job`, "--role-session-name", "nightly-report"]);
  await new Promise((r) => setTimeout(r, 1500));

  async function settle(p: Page) {
    await p.evaluate(() => document.fonts.ready);
    await p.waitForTimeout(700);
  }

  const pages: [string, string][] = [
    ["wire", "/live?view=wire"],
    ["queue", "/resources/sqs/queues/pick-lists"],
    ["topic", "/resources/sns/topics/order-placed"],
    ["bucket", "/resources/s3/buckets/product-images"],
    ["object", "/resources/s3/buckets/product-images?do=object&key=sku%2Ftomato.jpg"],
  ];
  for (const name of only.split(",") as (keyof typeof engines)[]) {
    const b = await engines[name].launch();
    mkdirSync(`${out}/${name}`, { recursive: true });
    for (const mode of ["light", "dark"] as const) {
      const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
      await ctx.addInitScript((m) => localStorage.setItem("doze-aws.theme", m), mode);
      const p = await ctx.newPage();
      for (const [label, path] of pages) {
        await p.goto(`${server.console}${path}`);
        await settle(p);
        const calls = p.getByTestId("recent-calls");
        if (await calls.count()) await calls.scrollIntoViewIfNeeded();
        await p.screenshot({ path: `${out}/${name}/${label}-${mode}.png` });
      }
      // The drawer of the publish.
      await p.goto(`${server.console}/live?view=wire`);
      await settle(p);
      await p.locator("tr[data-row]", { hasText: "Publish" }).first().click();
      await p.getByTestId("call-sheet").waitFor();
      await settle(p);
      await p.screenshot({ path: `${out}/${name}/drawer-${mode}.png` });
      await ctx.close();
    }
    await b.close();
  }
} finally {
  await server.stop();
}
