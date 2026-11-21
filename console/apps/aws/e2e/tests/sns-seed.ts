// The world the SNS boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: the queues the topics deliver to, a KMS key and an
// IAM role for the settings that name them, a Lambda function, and two topics: orders-created (three SQS queues, settings, tags) and alerts (a
// function, and an email address that waits for a confirmation no one is sent). The boards add a FIFO topic and a webhook, which is a real local
// server here, so the confirmation message SNS posts to it can be read and its SubscribeURL pasted.
// `bun e2e/tests/sns-seed.ts http://127.0.0.1:44700 [account]` seeds a running one by hand.
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

export const topicArn = (account: string, name: string) => `arn:aws:sns:us-east-1:${account}:${name}`;
export const queueArn = (account: string, name: string) => `arn:aws:sqs:us-east-1:${account}:${name}`;
const ROLE = (account: string) => `arn:aws:iam::${account}:role/sns-delivery-logs`;

/** A local webhook: what SNS posts to it is kept, and the SubscribeURL of a confirmation message is read out of it. */
export type Webhook = { url: string; posts: () => { type: string; body: Record<string, unknown> }[]; subscribeUrl: () => string | null; stop: () => void };

export function startWebhook(port: number): Webhook {
  const posts: { type: string; body: Record<string, unknown> }[] = [];
  const server = Bun.serve({
    port,
    hostname: "127.0.0.1",
    async fetch(req) {
      const text = await req.text();
      try {
        posts.push({ type: req.headers.get("x-amz-sns-message-type") ?? "", body: JSON.parse(text) as Record<string, unknown> });
      } catch {
        /* not JSON: not a message */
      }
      return new Response("ok");
    },
  });
  return {
    url: `http://127.0.0.1:${port}/in`,
    posts: () => posts,
    subscribeUrl: () => (posts.find((p) => p.type === "SubscriptionConfirmation")?.body.SubscribeURL as string | undefined) ?? null,
    stop: () => void server.stop(true),
  };
}

/** Makes the larder world; the account is the one the doze-aws serves. */
export function seedLarder(endpoint: string, account: string): void {
  const cli = runner(endpoint);
  const key = (JSON.parse(cli(["kms", "create-key", "--description", "Larder application data key"])) as { KeyMetadata: { KeyId: string } }).KeyMetadata.KeyId;
  cli(["kms", "create-alias", "--alias-name", "alias/larder-app", "--target-key-id", key]);
  const trust = (service: string) => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: service }, Action: "sts:AssumeRole" }] });
  cli(["iam", "create-role", "--role-name", "sns-delivery-logs", "--assume-role-policy-document", trust("sns.amazonaws.com")]);

  for (const n of ["invoice-queue", "inventory-queue", "analytics-queue", "pick-lists", "pick-lists-dlq", "email-outbox", "orders-events-dlq.fifo"]) {
    cli(["sqs", "create-queue", "--queue-name", n, ...(n.endsWith(".fifo") ? ["--attributes", JSON.stringify({ FifoQueue: "true" })] : [])]);
  }
  cli(["sqs", "create-queue", "--queue-name", "orders-events.fifo", "--attributes", JSON.stringify({ FifoQueue: "true", ContentBasedDeduplication: "false" })]);

  const dir = mkdtempSync(join(tmpdir(), "sns-seed-"));
  writeFileSync(join(dir, "index.py"), 'def handler(event, context):\n    return {"ok": True}\n');
  const zip = join(dir, "fn.zip");
  if (Bun.spawnSync(["zip", "-q", "-j", zip, join(dir, "index.py")]).exitCode !== 0) throw new Error("zip failed");
  cli(["iam", "create-role", "--role-name", "larder-fn", "--assume-role-policy-document", trust("lambda.amazonaws.com")]);
  cli(["lambda", "create-function", "--function-name", "send-email", "--runtime", "python3.13", "--role", `arn:aws:iam::${account}:role/larder-fn`, "--handler", "index.handler", "--timeout", "10", "--zip-file", `fileb://${zip}`]);

  const created = (JSON.parse(cli(["sns", "create-topic", "--name", "orders-created", "--attributes", JSON.stringify({ DisplayName: "larder orders", KmsMasterKeyId: "alias/larder-app", SQSSuccessFeedbackRoleArn: ROLE(account), SQSSuccessFeedbackSampleRate: "100", SQSFailureFeedbackRoleArn: ROLE(account) }), "--tags", "Key=stack,Value=larder", "Key=app,Value=orders"])) as { TopicArn: string }).TopicArn;
  const alerts = (JSON.parse(cli(["sns", "create-topic", "--name", "alerts"])) as { TopicArn: string }).TopicArn;
  for (const n of ["invoice-queue", "inventory-queue", "analytics-queue"]) cli(["sns", "subscribe", "--topic-arn", created, "--protocol", "sqs", "--notification-endpoint", queueArn(account, n)]);
  cli(["sns", "subscribe", "--topic-arn", alerts, "--protocol", "lambda", "--notification-endpoint", `arn:aws:lambda:us-east-1:${account}:function:send-email`]);
  cli(["sns", "subscribe", "--topic-arn", alerts, "--protocol", "email", "--notification-endpoint", "ops@larder.example"]);
}

if (import.meta.main) {
  const [endpoint = "http://127.0.0.1:44700", account = "111122223333", port = "44790"] = process.argv.slice(2);
  const hook = startWebhook(Number(port));
  seedLarder(endpoint, account);
  console.log(`seeded ${endpoint}; a webhook is at ${hook.url} for a subscription to confirm; Ctrl-C to stop it`);
}
