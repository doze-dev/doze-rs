// Screenshots of Resources for a person to look at: a real doze-aws of its own (ports from the environment, else 42110 and
// 42111), resources made with the real AWS CLI (queues with a dead-letter queue, a topic subscribed by a queue, a table, a
// bucket with objects, a secret, parameters, a key), then the index, a list, a detail, the create dialog and the delete
// confirmation, light and dark, in each browser.
//   bun e2e/resource-shots.ts <out dir> [firefox,webkit,chromium]
import { mkdirSync } from "node:fs";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws, start } from "./server";

const [out = "resource-shots", only = "firefox,webkit,chromium"] = process.argv.slice(2);
const listen = Number(process.env.DOZE_AWS_SHOTS_PORT ?? 42110);
const server = await start(listen, listen + 1);
const engines = { firefox, webkit, chromium };

function run(args: string[]) {
  const r = aws(server.endpoint, args, { accessKey: "AKIALARDERAPP0000001" });
  if (r.code !== 0) console.warn(`aws ${args.join(" ")}: ${r.out}`);
  return r;
}

try {
  const acct = server.account;
  run(["sqs", "create-queue", "--queue-name", "pick-lists-dlq"]);
  const dlq = `arn:aws:sqs:us-east-1:${acct}:pick-lists-dlq`;
  run(["sqs", "create-queue", "--queue-name", "pick-lists", "--attributes", JSON.stringify({ RedrivePolicy: JSON.stringify({ deadLetterTargetArn: dlq, maxReceiveCount: "5" }) })]);
  run(["sqs", "create-queue", "--queue-name", "email-outbox"]);
  run(["sqs", "create-queue", "--queue-name", "orders-events.fifo", "--attributes", "FifoQueue=true,ContentBasedDeduplication=true"]);
  const pick = `${server.endpoint}/${acct}/pick-lists`;
  for (let i = 0; i < 3; i++) run(["sqs", "send-message", "--queue-url", pick, "--message-body", `pick ${i}`]);
  run(["sqs", "receive-message", "--queue-url", pick]);
  run(["sns", "create-topic", "--name", "order-placed"]);
  run(["sns", "subscribe", "--topic-arn", `arn:aws:sns:us-east-1:${acct}:order-placed`, "--protocol", "sqs", "--notification-endpoint", `arn:aws:sqs:us-east-1:${acct}:pick-lists`]);
  run(["dynamodb", "create-table", "--table-name", "orders", "--attribute-definitions", "AttributeName=id,AttributeType=S", "AttributeName=at,AttributeType=N", "--key-schema", "AttributeName=id,KeyType=HASH", "AttributeName=at,KeyType=RANGE", "--billing-mode", "PAY_PER_REQUEST"]);
  run(["dynamodb", "put-item", "--table-name", "orders", "--item", '{"id":{"S":"o-1"},"at":{"N":"1"}}']);
  run(["s3api", "create-bucket", "--bucket", "product-images"]);
  run(["s3api", "put-object", "--bucket", "product-images", "--key", "sku/tomato.jpg", "--body", import.meta.filename]);
  run(["s3api", "put-object", "--bucket", "product-images", "--key", "sku/basil.jpg", "--body", import.meta.filename]);
  run(["secretsmanager", "create-secret", "--name", "larder/prod/stripe-key", "--description", "Stripe API key", "--secret-string", "sk_test_shots"]);
  run(["ssm", "put-parameter", "--name", "/larder/prod/db-url", "--type", "SecureString", "--value", "postgres://larder"]);
  run(["ssm", "put-parameter", "--name", "/larder/prod/region", "--type", "String", "--value", "us-east-1"]);
  run(["kms", "create-key", "--description", "larder app key"]);
  await new Promise((r) => setTimeout(r, 500));

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
      await p.goto(`${server.console}/resources`);
      await settle(p, "Resources");
      await p.getByTestId("resources-index").waitFor();
      await shot("index", true);
      await p.goto(`${server.console}/resources/sqs/queues`);
      await settle(p, "Queues");
      await shot("list");
      await p.getByTestId("create-open").click();
      await p.getByTestId("create-dialog").waitFor();
      await p.getByTestId("field-QueueName").fill("returns");
      await p.waitForTimeout(400);
      await shot("create");
      await p.keyboard.press("Escape");
      await p.goto(`${server.console}/resources/sqs/queues/pick-lists`);
      await settle(p, "pick-lists");
      await p.getByTestId("detail").waitFor();
      await shot("detail", true);
      await p.getByTestId("delete-open").click();
      await p.getByTestId("delete-dialog").waitFor();
      await p.waitForTimeout(400);
      await shot("delete");
      await p.keyboard.press("Escape");
      await p.goto(`${server.console}/resources/secretsmanager/secrets/${encodeURIComponent("larder/prod/stripe-key")}`);
      await settle(p, "larder/prod/stripe-key");
      await shot("secret");
      await p.goto(`${server.console}/resources/s3/buckets/product-images`);
      await settle(p, "product-images");
      await shot("bucket", true);
      await ctx.close();
    }
    await b.close();
  }
} finally {
  await server.stop();
}
