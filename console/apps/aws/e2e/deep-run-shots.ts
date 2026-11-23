// Screenshots of the deep pages (half B) for a person to look at: a real doze-aws of its own (ports from the environment, else
// 42320 and 42321), a function, a state machine, a log group and a stack made with the real AWS CLI, then each deep page, tab
// and dialog, light and dark, in each browser.
//   bun e2e/deep-shots.ts <out dir> [firefox,webkit,chromium]
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws, start } from "./server";

const [out = "deep-shots", only = "firefox,webkit,chromium"] = process.argv.slice(2);
const listen = Number(process.env.DOZE_AWS_SHOTS_PORT ?? 42320);
const server = await start(listen, listen + 1);
const engines = { firefox, webkit, chromium };
const enc = encodeURIComponent;

function run(args: string[]) {
  const r = aws(server.endpoint, args, { accessKey: "AKIALARDERAPP0000001" });
  if (r.code !== 0) console.warn(`aws ${args.join(" ")}: ${r.out}`);
  return r;
}

const BOOTSTRAP = `#!/bin/sh
while true; do
  H=$(mktemp)
  EVENT=$(curl -sS -D "$H" "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/next")
  ID=$(grep -i '^Lambda-Runtime-Aws-Request-Id' "$H" | cut -d: -f2 | tr -d ' \\r\\n')
  rm -f "$H"
  echo "sending email for $ID"
  case "$EVENT" in
    *fail*) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/error" -d '{"errorMessage":"no address on the order","errorType":"KeyError"}' ;;
    *) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/response" -d "{\\"sent\\":1,\\"event\\":$EVENT}" ;;
  esac
done
`;

const dir = join(tmpdir(), `doze-aws-deep-shots-${process.pid}`);
mkdirSync(dir, { recursive: true });

try {
  const acct = server.account;
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });
  run(["iam", "create-role", "--role-name", "lambda-exec", "--assume-role-policy-document", trust]);
  writeFileSync(join(dir, "bootstrap"), BOOTSTRAP, { mode: 0o755 });
  execFileSync("zip", ["-qj", join(dir, "f.zip"), join(dir, "bootstrap")]);
  run(["lambda", "create-function", "--function-name", "send-email", "--runtime", "provided.al2023", "--handler", "x", "--role", `arn:aws:iam::${acct}:role/lambda-exec`, "--zip-file", `fileb://${join(dir, "f.zip")}`, "--timeout", "30", "--memory-size", "256", "--environment", "Variables={LOG_LEVEL=INFO,FROM_ADDRESS=orders@larder.dev}", "--description", "Sends the order emails"]);
  run(["lambda", "publish-version", "--function-name", "send-email", "--description", "first cut"]);
  run(["lambda", "create-alias", "--function-name", "send-email", "--name", "live", "--function-version", "1"]);
  const payload = join(dir, "p.json");
  writeFileSync(payload, JSON.stringify({ order: "ord-0421" }));
  run(["lambda", "invoke", "--function-name", "send-email", "--cli-binary-format", "raw-in-base64-out", "--payload", `file://${payload}`, join(dir, "out.json")]);

  const machine = {
    StartAt: "LoadOrder",
    States: {
      LoadOrder: { Type: "Pass", Result: { email: "priya@example.com" }, ResultPath: "$.order", Next: "Route" },
      Route: { Type: "Choice", Choices: [{ Variable: "$.hold", IsPresent: true, Next: "WaitForStock" }], Default: "Fulfil" },
      WaitForStock: { Type: "Wait", Seconds: 600, Next: "Fulfil" },
      Fulfil: { Type: "Parallel", Branches: [{ StartAt: "CreatePickList", States: { CreatePickList: { Type: "Pass", End: true } } }, { StartAt: "ReserveStock", States: { ReserveStock: { Type: "Pass", Next: "ConfirmStock" }, ConfirmStock: { Type: "Pass", End: true } } }], Next: "NotifyCustomer" },
      NotifyCustomer: { Type: "Pass", End: true },
    },
  };
  const sm = run(["stepfunctions", "create-state-machine", "--name", "order-fulfilment", "--definition", JSON.stringify(machine), "--role-arn", `arn:aws:iam::${acct}:role/lambda-exec`]);
  const smArn = (JSON.parse(sm.out) as { stateMachineArn: string }).stateMachineArn;
  const done = run(["stepfunctions", "start-execution", "--state-machine-arn", smArn, "--name", "ord-20261121-0420", "--input", '{"order_id":"ord-0420"}']);
  const doneArn = (JSON.parse(done.out) as { executionArn: string }).executionArn;
  const held = run(["stepfunctions", "start-execution", "--state-machine-arn", smArn, "--name", "ord-20261121-0421", "--input", '{"order_id":"ord-0421","hold":true}']);
  const heldArn = (JSON.parse(held.out) as { executionArn: string }).executionArn;

  const group = "/larder/api";
  run(["logs", "create-log-group", "--log-group-name", group]);
  const now = Date.now() - 30 * 60_000;
  for (const [s, n] of [["payments-8c2e1", 40], ["orders-3f9a0", 25]] as const) {
    run(["logs", "create-log-stream", "--log-group-name", group, "--log-stream-name", s]);
    const lines = Array.from({ length: n }, (_, i) => ({ timestamp: now + i * 20_000, message: i % 13 === 7 ? JSON.stringify({ level: "error", msg: "card declined", order: `ord-04${i}` }) : i % 9 === 4 ? `WARN slow upstream: ${120 + i} ms` : JSON.stringify({ level: "info", msg: "request done", path: "/orders", ms: 12 + i }) }));
    const f = join(dir, `${s}.json`);
    writeFileSync(f, JSON.stringify(lines));
    run(["logs", "put-log-events", "--log-group-name", group, "--log-stream-name", s, "--log-events", `file://${f}`]);
  }

  const template = join(dir, "stack.yaml");
  writeFileSync(template, "Description: Larder's order pipeline\nResources:\n  Inbox:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: stack-inbox\n  Events:\n    Type: AWS::SNS::Topic\n    Properties:\n      TopicName: stack-events\nOutputs:\n  InboxUrl:\n    Value: !Ref Inbox\n");
  run(["cloudformation", "create-stack", "--stack-name", "larder-core", "--template-body", `file://${template}`]);
  await new Promise((r) => setTimeout(r, 1500));

  async function settle(p: Page, title: string) {
    await p.locator("h1", { hasText: title }).first().waitFor();
    await p.evaluate(() => document.fonts.ready);
    await p.waitForTimeout(600);
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
      const dialog = async (open: string, id: string, what: string) => {
        await p.getByTestId(open).click();
        await p.getByTestId(id).waitFor();
        await p.waitForTimeout(400);
        await shot(what);
        await p.keyboard.press("Escape");
        await p.waitForTimeout(300);
      };
      // Lambda
      await p.goto(`${server.console}/resources/lambda/functions/send-email?tab=test`);
      await settle(p, "send-email");
      await p.getByTestId("invoke-event").fill('{\n  "order": "ord-0421",\n  "email": "priya@example.com"\n}');
      await p.getByTestId("invoke-run").click();
      await p.getByTestId("invoke-result").waitFor();
      await p.waitForTimeout(300);
      await shot("lambda-test-ok", true);
      await p.getByTestId("invoke-event").fill('{"fail": true}');
      await p.getByTestId("invoke-run").click();
      await p.getByTestId("invoke-status").filter({ hasText: "Function error" }).waitFor();
      await p.waitForTimeout(300);
      await shot("lambda-test-error", true);
      await page("/resources/lambda/functions/send-email?tab=configuration", "send-email", "lambda-config");
      await dialog("config-edit", "config-dialog", "lambda-config-dialog");
      await dialog("env-edit", "env-dialog", "lambda-env-dialog");
      await page("/resources/lambda/functions/send-email?tab=versions", "send-email", "lambda-versions");
      await page("/resources/lambda/functions/send-email?tab=invocations", "send-email", "lambda-invocations");
      await page("/resources/lambda/functions/send-email", "send-email", "lambda-details", false);
      // Step Functions
      await page("/resources/stepfunctions/machines/order-fulfilment?tab=executions", "order-fulfilment", "sfn-executions");
      await dialog("start-open", "start-dialog", "sfn-start-dialog");
      await page(`/resources/stepfunctions/executions/${enc(doneArn)}`, "ord-20261121-0420", "sfn-execution-succeeded");
      await p.getByTestId("sfn-graph").locator('[data-state="ConfirmStock"]').click();
      await p.getByTestId("step-output").click();
      await p.waitForTimeout(300);
      await shot("sfn-execution-step", true);
      await p.getByTestId("tab-events").click();
      await p.getByTestId("history-open").nth(1).click();
      await p.waitForTimeout(300);
      await shot("sfn-execution-events", true);
      await page(`/resources/stepfunctions/executions/${enc(heldArn)}`, "ord-20261121-0421", "sfn-execution-running");
      await dialog("stop-open", "stop-dialog", "sfn-stop-dialog");
      // Logs
      await page(`/resources/logs/groups/${enc(group)}?tab=streams`, group, "logs-streams");
      await page(`/resources/logs/streams/payments-8c2e1?group=${enc(group)}`, "payments-8c2e1", "logs-stream-viewer");
      await p.getByTestId("log-event").filter({ hasText: "card declined" }).first().getByRole("button").click();
      await p.waitForTimeout(300);
      await shot("logs-stream-open", false);
      await p.getByTestId("stream-wrap").click();
      await p.waitForTimeout(200);
      await shot("logs-stream-nowrap", false);
      await page(`/resources/logs/groups/${enc(group)}?tab=search`, group, "logs-search-empty", false);
      await p.getByTestId("search-pattern").fill('{ $.level = "error" }');
      await p.getByTestId("search-run").click();
      await p.getByTestId("search-results").waitFor();
      await p.waitForTimeout(300);
      await shot("logs-search", true);
      await page(`/resources/logs/groups/${enc(group)}?tab=tail`, group, "logs-tail-off", false);
      await p.getByTestId("tail-start").click();
      await p.getByTestId("tail-status").filter({ hasText: "Tailing" }).waitFor();
      run(["logs", "put-log-events", "--log-group-name", group, "--log-stream-name", "orders-3f9a0", "--log-events", `timestamp=${Date.now()},message=ERROR order ord-0499 has no address`, `timestamp=${Date.now()},message=request done in 14 ms`]);
      await p.getByTestId("tail-events").waitFor({ timeout: 10_000 });
      await p.waitForTimeout(300);
      await shot("logs-tail", false);
      await p.getByTestId("tail-stop").click();
      // CloudFormation
      await page("/resources/cloudformation/stacks/larder-core", "larder-core", "cfn-details");
      await page("/resources/cloudformation/stacks/larder-core?tab=events", "larder-core", "cfn-events");
      await page("/resources/cloudformation/stacks/larder-core?tab=template", "larder-core", "cfn-template");
      await page("/resources/cloudformation/stacks/larder-core?tab=changesets", "larder-core", "cfn-changesets", false);
      await ctx.close();
    }
    await b.close();
  }
} finally {
  await server.stop();
}
