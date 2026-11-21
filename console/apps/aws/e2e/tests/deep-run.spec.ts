// The deep pages against a real doze-aws, each set up with the real AWS CLI: a function tested (its answer, its error, its
// log tail), configured in dialogs, its versions and its runs' log streams; a state machine's execution started from its page,
// drawn as a graph with the path it took, its events opened, and a running one stopped; a log stream read and paged, a group
// searched and tailed live; a stack's status sentence, events, template and change sets.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, type Running, start } from "../server";
import { LISTEN_PORT } from "../global-setup";
import { calls as allCalls, expect, test as base, unique } from "./fixtures";

// These tests run against a doze-aws of their own (the two ports after the shared one's): a function, a machine and a stack
// make calls on their callers' behalf (Lambda, Step Functions, CloudFormation), callers that would crowd the shared Live map
// and its filters other tests read.
let own: Running | undefined;
const test = base.extend<{ cli: (args: string[]) => { code: number; out: string } }>({
  cli: async ({ accessKey }, provide) => {
    await provide((args) => aws(own?.endpoint ?? "", args, { accessKey }));
  },
});
test.beforeAll(async () => {
  own = await start(LISTEN_PORT + 2, LISTEN_PORT + 3);
});
test.afterAll(async () => {
  await own?.stop();
});

/** Opens a page of this file's doze-aws and waits for its title. */
async function open(page: Page, path: string, title: string) {
  await page.goto(`${own?.console}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
}
const calls = (page: Page) => allCalls(page, own?.console);

/** A `provided` function in sh over the Runtime API (it needs curl): echoes its event and $MODE, fails when asked. */
const BOOTSTRAP = `#!/bin/sh
while true; do
  H=$(mktemp)
  EVENT=$(curl -sS -D "$H" "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/next")
  ID=$(grep -i '^Lambda-Runtime-Aws-Request-Id' "$H" | cut -d: -f2 | tr -d ' \\r\\n')
  rm -f "$H"
  echo "handling $ID"
  case "$EVENT" in
    *fail*) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/error" -d '{"errorMessage":"asked to fail","errorType":"Custom"}' ;;
    *) curl -sS -X POST "http://$AWS_LAMBDA_RUNTIME_API/2018-06-01/runtime/invocation/$ID/response" -d "{\\"echo\\":$EVENT,\\"mode\\":\\"$MODE\\"}" ;;
  esac
done
`;

/** The function's code, zipped with the zip program as a person would. */
function zipped(): { file: string; done: () => void } {
  const dir = mkdtempSync(join(tmpdir(), "doze-aws-e2e-fn-"));
  writeFileSync(join(dir, "bootstrap"), BOOTSTRAP, { mode: 0o755 });
  execFileSync("zip", ["-qj", join(dir, "f.zip"), join(dir, "bootstrap")]);
  return { file: join(dir, "f.zip"), done: () => rmSync(dir, { recursive: true, force: true }) };
}

const TRUST = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });

test("a function is tested, configured on pages of its own, and its runs' logs open in the stream viewer", async ({ page, cli, browserName }) => {
  const fn = unique("echo", browserName);
  const role = unique("fn-role", browserName);
  expect(cli(["iam", "create-role", "--role-name", role, "--assume-role-policy-document", TRUST]).code).toBe(0);
  const zip = zipped();
  try {
    const made = cli(["lambda", "create-function", "--function-name", fn, "--runtime", "provided.al2023", "--handler", "x", "--role", `arn:aws:iam::111122223333:role/${role}`, "--zip-file", `fileb://${zip.file}`, "--timeout", "10"]);
    expect(made.code, made.out).toBe(0);
  } finally {
    zip.done();
  }
  await open(page, `/resources/lambda/functions/${fn}`, fn);
  await page.getByTestId("tab-test").click();
  await expect(page).toHaveURL(/tab=test/);
  await page.getByTestId("invoke-event").fill('{"n": 1}');
  await page.getByTestId("invoke-run").click();
  await expect(page.getByTestId("invoke-status")).toContainText("Status: Succeeded", { timeout: 20_000 });
  await expect(page.getByTestId("invoke-payload")).toContainText('"n": 1');
  await expect(page.getByTestId("invoke-log")).toContainText("START RequestId");
  await expect(page.getByTestId("invoke-report")).toContainText("Billed Duration");
  // Not JSON: refused before any call.
  await page.getByTestId("invoke-event").fill("{n:");
  await expect(page.getByTestId("invoke-event-help")).toContainText("Not JSON");
  await expect(page.getByTestId("invoke-run")).toBeDisabled();
  await page.getByTestId("invoke-event").fill('{"fail": true}');
  await page.getByTestId("invoke-run").click();
  await expect(page.getByTestId("invoke-status")).toContainText("The function returned an error.");
  await expect(page.getByTestId("invoke-payload")).toContainText("asked to fail");

  await page.getByTestId("tab-configuration").click();
  await page.getByTestId("config-edit").click();
  await page.getByTestId("cfg-memory").fill("256");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("was saved");
  await expect(page.getByTestId("general-card")).toContainText("256 MB");
  await page.getByTestId("section-env").click();
  await page.getByTestId("env-edit").click();
  await page.getByRole("button", { name: "Add environment variable" }).click();
  await page.getByTestId("env-kv-key-0").fill("1BAD");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("env-problem-0")).toContainText("A key starts with a letter");
  await page.getByTestId("env-kv-key-0").fill("MODE");
  await page.getByTestId("env-kv-value-0").fill("blue");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("environment variables");
  await expect(page.getByTestId("env-card")).toContainText("blue");

  await page.getByTestId("tab-versions").click();
  await expect(page.getByTestId("lambda-versions")).toContainText("$LATEST");
  await page.getByTestId("tab-aliases").click();
  await expect(page.getByTestId("aliases-none")).toBeVisible();

  await page.reload();
  await page.getByTestId("tab-monitor").click();
  await page.getByTestId("view-logs").click();
  await page.getByTestId("tab-streams").click();
  const stream = page.getByTestId("group-streams").getByTestId("stream-link").first();
  await stream.click();
  await expect(page.getByTestId("stream-viewer")).toBeVisible();
  await expect(page.getByTestId("log-event").first()).toBeVisible();
  await expect(page.getByTestId("log-events")).toContainText("handling");
  const done = (await calls(page)).calls.filter((c) => c.caller === "console" && c.action === "Invoke");
  expect(done.length).toBeGreaterThanOrEqual(2);
});

const MACHINE = {
  StartAt: "Route",
  States: {
    Route: { Type: "Choice", Choices: [{ Variable: "$.wait", IsPresent: true, Next: "Hold" }], Default: "Fan" },
    Hold: { Type: "Wait", Seconds: 300, Next: "Done" },
    Fan: { Type: "Parallel", Branches: [{ StartAt: "Left", States: { Left: { Type: "Pass", End: true } } }, { StartAt: "Right", States: { Right: { Type: "Pass", Result: { side: "right" }, End: true } } }], Next: "Done" },
    Done: { Type: "Succeed" },
  },
};

test("an execution is started from its machine, drawn with the path it took, and a running one is stopped", async ({ page, cli, browserName }) => {
  const name = unique("flow", browserName);
  const made = cli(["stepfunctions", "create-state-machine", "--name", name, "--definition", JSON.stringify(MACHINE), "--role-arn", "arn:aws:iam::111122223333:role/sfn"]);
  expect(made.code, made.out).toBe(0);
  await open(page, `/resources/stepfunctions/machines/${name}?tab=executions`, name);
  await page.getByTestId("verb-start").click();
  const start = page.getByTestId("start-dialog");
  await start.getByTestId("start-name").fill("quick");
  await start.getByTestId("start-input").fill('{"order": 1}');
  await start.getByTestId("start-submit").click();
  await expect(page.locator("h1")).toHaveText("quick");
  await expect(async () => {
    await page.getByTestId("refresh-now").click();
    await expect(page.getByTestId("execution-status")).toHaveText("Succeeded", { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  const graph = page.getByTestId("sfn-graph");
  await expect(graph.locator('[data-state="Fan"]')).toHaveAttribute("data-status", "succeeded");
  await expect(graph.locator('[data-state="Right"]')).toHaveAttribute("data-status", "succeeded");
  await expect(graph.locator('[data-state="Hold"]')).toHaveAttribute("data-status", "idle");
  await expect(graph.locator('[data-edge="Route>Fan"]')).toHaveAttribute("data-taken", "true");
  await graph.locator('[data-state="Right"]').click();
  await page.getByTestId("step-views").getByRole("radio", { name: "Output" }).click();
  await expect(page.getByTestId("step-details")).toContainText('"side": "right"');
  await page.getByTestId("tab-events").click();
  const events = page.getByTestId("history-event");
  await expect(events.first()).toContainText("ExecutionStarted");
  await page.getByTestId("history-sort").click();
  await expect(events.first()).toContainText("ExecutionSucceeded");
  await page.getByTestId("history-open").first().click();
  await expect(page.getByTestId("history")).toContainText('"output"');

  await page.getByTestId("start-open").click();
  await page.getByTestId("start-dialog").getByTestId("start-input").fill('{"wait": true}');
  await page.getByTestId("start-dialog").getByTestId("start-submit").click();
  await expect(page.getByTestId("execution-status")).toHaveText("Running");
  await expect(page.getByTestId("sfn-graph").locator('[data-state="Hold"]')).toHaveAttribute("data-status", "running");
  await page.getByTestId("stop-open").click();
  await page.getByTestId("stop-dialog").getByTestId("stop-submit").click();
  await expect(page.getByTestId("execution-status")).toHaveText("Aborted");
  await expect(page.getByTestId("stop-open")).toHaveCount(0);
  const stops = (await calls(page)).calls.filter((c) => c.caller === "console" && c.action === "StopExecution");
  expect(stops.map((c) => c.outcome)).toContain("ok");
});

test("a log stream is read and paged, its group searched, and tailed live", async ({ page, cli, browserName }) => {
  const group = `/e2e/${unique("deep", browserName)}`;
  expect(cli(["logs", "create-log-group", "--log-group-name", group]).code).toBe(0);
  expect(cli(["logs", "create-log-stream", "--log-group-name", group, "--log-stream-name", "web-1"]).code).toBe(0);
  const now = Date.now() - 60_000;
  const lines = Array.from({ length: 120 }, (_, i) => ({ timestamp: now + i, message: i === 7 ? 'ERROR {"order":7,"level":"error"}' : `line ${i}` }));
  const file = join(mkdtempSync(join(tmpdir(), "doze-aws-e2e-logs-")), "events.json");
  writeFileSync(file, JSON.stringify(lines));
  expect(cli(["logs", "put-log-events", "--log-group-name", group, "--log-stream-name", "web-1", "--log-events", `file://${file}`]).code).toBe(0);
  expect(readFileSync(file, "utf8").length).toBeGreaterThan(0);
  await open(page, `/resources/logs/groups/${encodeURIComponent(group)}?tab=streams`, group);
  await page.getByTestId("group-streams").getByTestId("stream-link").click();
  await expect(page.locator("h1")).toHaveText("web-1");
  // The first 100 of 120 in the last hour, oldest first; the rest are loaded when asked for.
  await expect(page.getByTestId("log-event")).toHaveCount(100);
  await expect(page.getByTestId("log-events")).toContainText("line 0");
  await page.getByTestId("stream-more").click();
  await expect(page.getByTestId("log-event")).toHaveCount(120);
  await expect(page.getByTestId("log-events")).toContainText("line 119");
  await expect(page.locator('[data-testid="log-event"][data-level="bad"]')).toHaveCount(1);

  await open(page, `/resources/logs/groups/${encodeURIComponent(group)}?view=search`, "Search log group");
  await page.getByTestId("search-pattern").fill("ERROR");
  await page.getByTestId("search-run").click();
  await expect(page.getByTestId("search-count")).toContainText("1 event in the last hour matches the pattern");
  await expect(page.getByTestId("search-events")).toContainText("web-1");

  await open(page, `/resources/logs/groups/${encodeURIComponent(group)}?view=tail`, "Live Tail");
  await page.getByTestId("tail-start").click();
  await expect(page.getByTestId("tail-status")).toHaveText("Tailing");
  const live = [{ timestamp: Date.now(), message: `live ${browserName}` }];
  writeFileSync(file, JSON.stringify(live));
  expect(cli(["logs", "put-log-events", "--log-group-name", group, "--log-stream-name", "web-1", "--log-events", `file://${file}`]).code).toBe(0);
  await expect(page.getByTestId("tail-events")).toContainText(`live ${browserName}`, { timeout: 10_000 });
  await page.getByTestId("tail-stop").click();
  await expect(page.getByTestId("tail-status")).toHaveText("Not started");
  const tails = (await calls(page)).calls.filter((c) => c.caller === "console" && c.action === "StartLiveTail");
  expect(tails.length).toBeGreaterThanOrEqual(1);
});

test("a stack says where it stands, and shows its events, template and change sets", async ({ page, cli, browserName }) => {
  const stack = unique("deep-stack", browserName);
  const queue = unique("deep-inbox", browserName);
  const template = JSON.stringify({ Resources: { Inbox: { Type: "AWS::SQS::Queue", Properties: { QueueName: queue } } } });
  expect(cli(["cloudformation", "create-stack", "--stack-name", stack, "--template-body", template]).code).toBe(0);
  await expect(async () => {
    await open(page, `/resources/cloudformation/stacks/${stack}`, stack);
    await expect(page.getByTestId("stack-sentence")).toContainText("CREATE_COMPLETE", { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await expect(page.getByTestId("stack-sentence")).toContainText("not updated since");
  await page.getByTestId("tab-events").click();
  const events = page.getByTestId("stack-events").locator("tbody tr");
  await expect(events.first()).toContainText(stack);
  await expect(events.first()).toContainText("CREATE_COMPLETE");
  await expect(page.getByTestId("stack-events")).toContainText("AWS::SQS::Queue");
  await page.getByTestId("tab-template").click();
  await expect(page.getByTestId("template-body")).toContainText(queue);
  await page.getByTestId("tab-changesets").click();
  await expect(page.getByTestId("changesets-none")).toBeVisible();
});

test.describe("a stack that is gone", () => {
  // The page of a deleted stack's name reads a refused DescribeStacks (400), which the browser logs.
  test.use({ allowErrors: /status of 400|400 \(Bad Request\)/ });

test("a stack whose create failed holds nothing and says which resource failed; a deleted stack's name says it was deleted", async ({ page, cli, browserName }) => {
  const stack = unique("deep-failed", browserName);
  const clash = `/e2e/${unique("clash", browserName)}`;
  expect(cli(["ssm", "put-parameter", "--name", clash, "--type", "String", "--value", "x"]).code).toBe(0);
  const template = JSON.stringify({ Resources: { Taken: { Type: "AWS::SSM::Parameter", Properties: { Name: clash, Type: "String", Value: "y" } } } });
  expect(cli(["cloudformation", "create-stack", "--stack-name", stack, "--template-body", template]).code).toBe(0);
  await expect(async () => {
    await open(page, `/resources/cloudformation/stacks/${stack}`, stack);
    await expect(page.getByTestId("stack-sentence")).toContainText("ROLLBACK_COMPLETE", { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await expect(page.getByTestId("stack-sentence")).toContainText("holds nothing");
  await expect(page.getByTestId("stack-sentence")).not.toContainText("held before");
  await expect(page.getByTestId("stack-cause")).toContainText("Taken (AWS::SSM::Parameter) failed to create");
  // Deleted: its name is no longer answered; the page says it was deleted, not "ValidationError".
  expect(cli(["cloudformation", "delete-stack", "--stack-name", stack]).code).toBe(0);
  await expect(async () => {
    await open(page, `/resources/cloudformation/stacks/${stack}`, stack);
    await expect(page.getByTestId("stack-deleted")).toContainText("was deleted", { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await expect(page.getByTestId("detail-error")).toHaveCount(0);
  await open(page, `/resources/cloudformation/stacks/${stack}-never`, `${stack}-never`);
  await expect(page.getByTestId("stack-none")).toContainText("There is no stack named");
});
});
