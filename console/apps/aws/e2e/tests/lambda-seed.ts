// The world the Lambda boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: a role, four queues and two
// topics, four functions (send-email with environment variables, three versions, two aliases, an SNS trigger and an SQS mapping;
// resize-product-image and export-report as the other rows of the list; dispatch-hook with nothing at all) and a layer with three
// versions. `bun e2e/tests/lambda-seed.ts http://127.0.0.1:44990 [account]` seeds a running one by hand.
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

/** What send-email runs, as the Code tab board shows it. */
export const SEND_EMAIL = `import json, os, logging

log = logging.getLogger()
log.setLevel(os.environ.get("LOG_LEVEL", "INFO"))

def lambda_handler(event, context):
    # one SNS record per alert, one SQS record per queued mail
    record = event["Records"][0]
    subject = record["Sns"]["Subject"]
    log.info("sending alert %s to 1 recipient", subject)
    return {"sent": 1, "to": [os.environ["MAIL_FROM"]]}
`;

const NODE = `export const handler = async (event) => {\n  return { resized: true };\n};\n`;
const PLAIN = `def lambda_handler(event, context):\n    return {"ok": True}\n`;

export const arnOf = (account: string, service: string, name: string) => `arn:aws:${service}:us-east-1:${account}:${name}`;

/** A zip of one file. */
export function zipOf(file: string, source: string): string {
  const dir = mkdtempSync(join(tmpdir(), "lambda-seed-"));
  writeFileSync(join(dir, file), source);
  const zip = join(dir, "fn.zip");
  const made = Bun.spawnSync(["zip", "-q", "-j", zip, join(dir, file)]);
  if (made.exitCode !== 0) throw new Error("zip failed");
  return zip;
}

export function seedLarder(endpoint: string, account: string): void {
  const cli = runner(endpoint);
  const role = `arn:aws:iam::${account}:role/lambda-exec`;
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });
  cli(["iam", "create-role", "--role-name", "lambda-exec", "--assume-role-policy-document", trust]);
  cli(["iam", "create-role", "--role-name", "report-writer", "--assume-role-policy-document", trust]);

  for (const q of ["email-outbox", "invoice-queue", "analytics-queue"]) cli(["sqs", "create-queue", "--queue-name", q, ...(q === "email-outbox" ? ["--attributes", JSON.stringify({ VisibilityTimeout: "180" })] : [])]);
  cli(["sqs", "create-queue", "--queue-name", "orders-events.fifo", "--attributes", JSON.stringify({ FifoQueue: "true", ContentBasedDeduplication: "true" })]);
  const topic = (n: string) => (JSON.parse(cli(["sns", "create-topic", "--name", n])) as { TopicArn: string }).TopicArn;
  const alerts = topic("alerts");
  const created = topic("orders-created");
  cli(["sns", "subscribe", "--topic-arn", created, "--protocol", "sqs", "--notification-endpoint", arnOf(account, "sqs", "invoice-queue")]);

  const make = (name: string, runtime: string, handler: string, file: string, source: string, extra: string[] = []) =>
    cli(["lambda", "create-function", "--function-name", name, "--runtime", runtime, "--role", role, "--handler", handler, "--zip-file", `fileb://${zipOf(file, source)}`, ...extra]);
  make("send-email", "python3.13", "lambda_function.lambda_handler", "lambda_function.py", SEND_EMAIL, ["--description", "Send alert and invoice emails", "--memory-size", "256", "--timeout", "10", "--environment", "Variables={LOG_LEVEL=INFO,MAIL_FROM=alerts@larder.dev,SMTP_HOST=smtp.larder.dev}"]);
  make("resize-product-image", "nodejs22.x", "index.handler", "index.mjs", NODE, ["--description", "Resize product photos into thumbnails", "--memory-size", "512", "--timeout", "60"]);
  make("export-report", "python3.13", "lambda_function.lambda_handler", "lambda_function.py", PLAIN, ["--description", "Nightly sales export to the reports bucket", "--role", `arn:aws:iam::${account}:role/report-writer`, "--timeout", "120"]);
  make("dispatch-hook", "python3.13", "lambda_function.lambda_handler", "lambda_function.py", PLAIN);

  // send-email's history: three versions (each differs from the one before: Lambda publishes only a change) and two aliases.
  const update = (...args: string[]) => cli(["lambda", "update-function-configuration", "--function-name", "send-email", ...args]);
  cli(["lambda", "publish-version", "--function-name", "send-email", "--description", "first deploy"]);
  update("--timeout", "20");
  cli(["lambda", "publish-version", "--function-name", "send-email", "--description", "retry on SMTP timeout"]);
  cli(["lambda", "update-function-code", "--function-name", "send-email", "--zip-file", `fileb://${zipOf("lambda_function.py", `${SEND_EMAIL}\n# html template\n`)}`]);
  cli(["lambda", "publish-version", "--function-name", "send-email", "--description", "html alert template"]);
  cli(["lambda", "create-alias", "--function-name", "send-email", "--name", "live", "--function-version", "2", "--description", "Production traffic"]);
  cli(["lambda", "create-alias", "--function-name", "send-email", "--name", "staging", "--function-version", "3", "--description", "Next release"]);
  // $LATEST moves on after version 3: its code changes and its timeout is the 30 seconds the boards say.
  cli(["lambda", "update-function-code", "--function-name", "send-email", "--zip-file", `fileb://${zipOf("lambda_function.py", SEND_EMAIL)}`]);
  update("--timeout", "30");
  // Its asynchronous invocations have a configuration already (retries), so adding a destination changes it with Update.
  cli(["lambda", "put-function-event-invoke-config", "--function-name", "send-email", "--maximum-retry-attempts", "1"]);

  // Triggers: the alerts topic invokes it (a statement and a subscription), and a queue is mapped to it.
  cli(["lambda", "add-permission", "--function-name", "send-email", "--statement-id", "sns-alerts", "--action", "lambda:InvokeFunction", "--principal", "sns.amazonaws.com", "--source-arn", alerts]);
  cli(["sns", "subscribe", "--topic-arn", alerts, "--protocol", "lambda", "--notification-endpoint", arnOf(account, "lambda", "function:send-email")]);
  cli(["lambda", "create-event-source-mapping", "--function-name", "send-email", "--event-source-arn", arnOf(account, "sqs", "email-outbox"), "--batch-size", "10"]);

  // Layers: larder-shared, three versions.
  for (const n of [1, 2, 3]) {
    cli(["lambda", "publish-layer-version", "--layer-name", "larder-shared", "--description", "Logging and mail helpers", "--zip-file", `fileb://${zipOf("helpers.py", `# helpers ${n}\n`)}`, "--compatible-runtimes", "python3.13", "nodejs22.x", "--compatible-architectures", "x86_64"]);
  }

  // Runs: the alerts topic invokes it (Invoke (Event) from SNS), and the CLI invokes it with an event that works and one that does not.
  cli(["sns", "publish", "--topic-arn", alerts, "--subject", "dlq-not-empty", "--message", "pick-lists-dlq has 1 message waiting"]);
  invoke(endpoint, "send-email", { Records: [{ Sns: { Subject: "dlq-not-empty" } }] });
  invoke(endpoint, "send-email", {});
}

/** Invokes `name` with `event` through the CLI; the CLI's own output (the status and the function error). */
export function invoke(endpoint: string, name: string, event: unknown): string {
  const dir = mkdtempSync(join(tmpdir(), "lambda-invoke-"));
  const r = aws(endpoint, ["lambda", "invoke", "--function-name", name, "--cli-binary-format", "raw-in-base64-out", "--payload", JSON.stringify(event), join(dir, "out.json")]);
  return r.out;
}

if (import.meta.main) {
  const [endpoint = "http://127.0.0.1:44990", account = "111122223333"] = process.argv.slice(2);
  seedLarder(endpoint, account);
  console.log(`seeded ${endpoint}`);
}
