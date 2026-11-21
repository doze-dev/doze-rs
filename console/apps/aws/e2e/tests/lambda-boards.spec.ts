// The 37 Lambda boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder" world
// with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { start, type Running, aws } from "../server";
import { CUT, expect, test } from "./fixtures";
import { invoke, seedLarder, zipOf } from "./lambda-seed";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-lambda");
const BASE = 44940;
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });
// Several boards are the service's refusal (a throttled call, a function that has no policy): the browser logs each 4xx as a failed load.
test.use({ allowErrors: CUT });

let server: Running;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

/** Photographs the page as the board is: light, then dark (the class the theme switch sets), in Chromium only. */
async function shot(page: Page, board: string) {
  if (browserKind !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
  const height = await page.evaluate(() => Math.max(document.documentElement.scrollHeight, window.innerHeight));
  await page.setViewportSize({ width: 1440, height });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(SHOTS, `${board}.light.png`) });
  await page.evaluate(() => document.documentElement.classList.add("dark"));
  await page.waitForTimeout(250);
  await page.screenshot({ path: join(SHOTS, `${board}.dark.png`) });
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await page.setViewportSize({ width: 1440, height: 900 });
}

const go = async (page: Page, path: string, title: string | RegExp) => {
  await page.goto(`${server.console}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
};
const fn = (name: string, search = "") => `/resources/lambda/functions/${name}${search}`;
const code = (page: Page) => page.getByTestId("call-panel-code");
const choose = async (page: Page, combobox: string, option: string | RegExp) => {
  await page.getByRole("combobox", { name: combobox }).click();
  await page.getByRole("option", { name: option }).click();
};

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  seedLarder(server.endpoint, server.account);
});
test.afterAll(async () => {
  await server.stop();
});

test("lambda__functions-list: the functions, a sentence of what ran, Create and Layers", async ({ page }) => {
  await go(page, "/resources/lambda/functions", "Event Functions");
  await expect(page.getByTestId("list-sentence")).toContainText("4 functions run here.");
  await expect(page.getByTestId("list-sentence")).toContainText("resize-product-image was invoked 4 times in the last hour, send-email 3 times, and export-report once.");
  const table = page.getByRole("table", { name: "Functions" });
  await expect(table.locator("tbody tr[data-row]")).toHaveCount(4);
  await expect(table.locator("tr[data-row]", { hasText: "send-email" })).toContainText("Send alert and invoice emails");
  await expect(table.locator("tr[data-row]", { hasText: "send-email" })).toContainText("Python 3.13");
  await expect(table.locator("tr[data-row]", { hasText: "resize-product-image" })).toContainText("Node.js 22.x");
  await expect(table.locator("tr[data-row]", { hasText: "send-email" })).toContainText("Zip");
  await expect(page.getByTestId("create-open")).toHaveText("Create Event Function");
  await expect(page.getByTestId("layers-link")).toBeVisible();
  await expect(page.getByTestId("page-line")).toHaveText("1 to 4 of 4 functions");
  await shot(page, "lambda__functions-list");
  await page.getByTestId("list-search").fill("export");
  await page.getByTestId("list-search-go").click();
  await expect(table.locator("tbody tr[data-row]")).toHaveCount(1);
  await page.getByTestId("search-clear").click();
  await expect(table.locator("tbody tr[data-row]")).toHaveCount(4);
});

test("lambda__function-create and lambda__function-created: the form, the call it makes, and the function it lands on", async ({ page }) => {
  await go(page, "/resources/lambda/functions?create=yes", "Create Event Function");
  await page.getByTestId("fn-name").fill("send-email");
  await expect(page.getByText("send-email already exists in this Region.")).toBeVisible();
  await page.getByTestId("fn-name").fill("weekly-digest");
  await choose(page, "Choose a role", "lambda-exec");
  await expect(code(page)).toContainText('"FunctionName": "weekly-digest"');
  await expect(code(page)).toContainText('"Runtime": "python3.13"');
  await expect(code(page)).toContainText('"Handler": "lambda_function.lambda_handler"');
  await expect(code(page)).toContainText("<the Hello world example, zipped>");
  await expect(code(page)).toContainText(`arn:aws:iam::${server.account}:role/lambda-exec`);
  await page.getByTestId("call-panel-tab-cli").click();
  await expect(code(page)).toContainText("lambda create-function --function-name weekly-digest");
  await page.getByTestId("call-panel-tab-json").click();
  await expect(page.getByTestId("author-off")).toHaveCount(2);
  await expect(page.getByTestId("role-create-off")).toContainText("Not in this form");
  await shot(page, "lambda__function-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("function-page")).toBeVisible();
  await expect(page.getByTestId("done-banner")).toContainText("weekly-digest was created.");
  await expect(page.getByTestId("detail-sentence")).toContainText("weekly-digest was created a moment ago with the Hello world example. It has no trigger yet, and runs as lambda-exec.");
  await expect(page.getByTestId("code-editor")).toHaveValue(/Hello from Lambda!/);
  await expect(page.getByTestId("code-file")).toContainText(/lambda_function\.py/i);
  await expect(page.getByTestId("code-explorer")).toContainText("weekly-digest");
  await shot(page, "lambda__function-created");
  // The function made from the page runs: the console's zip is one Lambda unpacks.
  const out = invoke(server.endpoint, "weekly-digest", {});
  expect(out).toContain('"StatusCode": 200');
  expect(out).not.toContain("FunctionError");
});

test("lambda__function-code: the code, its explorer, the environment, the runtime and the layers", async ({ page }) => {
  await go(page, fn("send-email"), "send-email");
  await expect(page.getByTestId("detail-sentence")).toContainText("send-email runs Python 3.13 with 256 MB and a 30 second timeout. SNS topic alerts and SQS queue email-outbox trigger it; it last ran at");
  await expect(page.getByTestId("detail-badge")).toHaveText(["Zip", "Active"]);
  await expect(page.getByTestId("wired-strip")).toContainText("Triggered by");
  await expect(page.getByTestId("wired-strip")).toContainText("SNS topic alerts");
  await expect(page.getByTestId("wired-strip")).toContainText("SQS queue email-outbox");
  await expect(page.getByTestId("wired-strip")).toContainText("role lambda-exec");
  await expect(page.getByTestId("wired-self")).toHaveText("send-email");
  for (const verb of ["trigger", "destination", "delete"]) await expect(page.getByTestId(`verb-${verb}`)).toBeVisible();
  await expect(page.getByTestId("code-editor")).toHaveValue(/def lambda_handler\(event, context\):/);
  await expect(page.getByTestId("code-file")).toContainText(/lambda_function\.py/i);
  await expect(page.getByTestId("code-env").locator("dt")).toHaveText(["LOG_LEVEL", "MAIL_FROM", "SMTP_HOST"]);
  await expect(page.getByTestId("runtime-settings")).toContainText("lambda_function.lambda_handler");
  await expect(page.getByTestId("runtime-settings")).toContainText("x86_64");
  await expect(page.getByTestId("layers-none")).toContainText("send-email does not use a layer. larder-shared version 3 is the newest in this account.");
  await expect(page.getByTestId("code-deploy")).toBeDisabled();
  await expect(page.getByTestId("recent-calls")).toBeVisible();
  await shot(page, "lambda__function-code");
});

test("lambda__function-delete: the dialog says what goes with the function and what stays", async ({ page }) => {
  await go(page, fn("send-email"), "send-email");
  await page.getByTestId("verb-delete").click();
  const dialog = page.getByTestId("delete-dialog");
  await expect(dialog).toContainText("Deleting send-email permanently deletes the function, its 3 versions and its 2 aliases (live, staging). A deleted function cannot be recovered.");
  await expect(page.getByTestId("delete-mappings")).toContainText("The event source mapping that reads email-outbox is not deleted with the function. Delete it separately.");
  await expect(dialog).toContainText("To confirm, type confirm");
  await expect(page.getByTestId("delete-dialog-submit")).toBeDisabled();
  await page.getByTestId("delete-dialog-input").fill("confirm");
  await expect(page.getByTestId("delete-dialog-submit")).toBeEnabled();
  await shot(page, "lambda__function-delete");
  await page.keyboard.press("Escape");
});

test("lambda__function-test and lambda__test-result-ok: an event shaped like its trigger, and what ran", async ({ page }) => {
  await go(page, fn("send-email", "?tab=test"), "send-email");
  await expect(page.getByTestId("invoke-template")).toContainText("SNS topic alerts");
  await expect(page.getByText("The event below has the shape the alerts topic sends to send-email.")).toBeVisible();
  await expect(page.getByTestId("invoke-event")).toHaveValue(/"EventSource": "aws:sns"/);
  await expect(page.getByTestId("invoke-call-code")).toContainText('"InvocationType": "RequestResponse"');
  await expect(page.getByTestId("invoke-call-code")).toContainText('"LogType": "Tail"');
  await shot(page, "lambda__function-test");
  await page.getByTestId("invoke-run").click();
  await expect(page.getByTestId("invoke-status")).toContainText("Status: Succeeded");
  await expect(page.getByTestId("invoke-payload")).toContainText('"sent": 1');
  await expect(page.getByTestId("invoke-log")).toContainText("START RequestId:");
  await expect(page.getByTestId("invoke-log")).toContainText("REPORT RequestId:");
  const report = page.getByTestId("invoke-report");
  await expect(report).toContainText("Request ID");
  await expect(report).toContainText("Billed Duration");
  await expect(report).toContainText("Memory Size256 MB");
  await shot(page, "lambda__test-result-ok");
});

test("lambda__test-result-error: the function threw, the call still answered 200", async ({ page }) => {
  await go(page, fn("send-email", "?tab=test"), "send-email");
  // The template is read from the function's triggers: wait for it, so nothing re-draws the editor under the typing.
  await expect(page.getByTestId("invoke-template")).toContainText("SNS topic alerts");
  await page.getByTestId("invoke-event").fill("{}");
  await expect(page.getByTestId("invoke-event")).toHaveValue("{}");
  await expect(page.getByTestId("invoke-run")).toBeEnabled();
  await page.getByTestId("invoke-run").click();
  await expect(page.getByTestId("invoke-status")).toHaveText("The function returned an error. The response is an error object instead of a result.");
  await expect(page.getByTestId("invoke-payload")).toContainText('"errorType": "KeyError"');
  await expect(page.getByTestId("invoke-error-teach")).toContainText("X-Amz-Function-Error");
  await expect(page.getByTestId("invoke-error-teach")).toContainText("Unhandled");
  await expect(page.getByTestId("invoke-report")).toContainText("FunctionErrorUnhandled");
  await shot(page, "lambda__test-result-error");
});

test("lambda__function-monitor: four metrics from AWS/Lambda and where the logs are", async ({ page }) => {
  await go(page, fn("send-email", "?tab=monitor"), "send-email");
  await expect(page.getByTestId("metric-invocations-value")).toHaveText("5");
  await expect(page.getByTestId("metric-errors-value")).toHaveText("2");
  await expect(page.getByTestId("metric-throttles-value")).toHaveText("0");
  await expect(page.getByTestId("metric-duration-value")).toContainText("ms");
  await expect(page.getByTestId("metric-invocations")).toContainText("5 in the last hour");
  await expect(page.getByTestId("metric-errors")).toContainText("2 errors in 5 invocations");
  await expect(page.getByTestId("logs-log-group")).toHaveText("/aws/lambda/send-email");
  await expect(page.getByTestId("logs-lines-per-invocation")).toContainText("START, END and REPORT");
  await shot(page, "lambda__function-monitor");
  await page.getByTestId("view-logs").click();
  await expect(page.locator("h1").first()).toHaveText("/aws/lambda/send-email");
});

test("lambda__test-result-throttled: reserved concurrency 0 refuses the call before it reaches the code", async ({ page }) => {
  cli(["lambda", "put-function-concurrency", "--function-name", "send-email", "--reserved-concurrent-executions", "0"]);
  await go(page, fn("send-email", "?tab=test"), "send-email");
  await page.getByTestId("invoke-run").click();
  await expect(page.getByTestId("invoke-refused-words")).toContainText("Invoke returned TooManyRequestsException. The request throughput limit was exceeded. The function did not run.");
  await expect(page.getByTestId("invoke-refused-details")).toContainText("HTTP status429");
  await expect(page.getByTestId("invoke-refused-details")).toContainText("ErrorTooManyRequestsException");
  await expect(page.getByTestId("invoke-refused-teach")).toContainText("A throttled call never reaches your code. Throttled requests do not count as Invocations or Errors.");
  await expect(page.getByTestId("invoke-refused-teach")).toContainText("reserved concurrency to 0 throttles it on purpose");
  await shot(page, "lambda__test-result-throttled");
  cli(["lambda", "delete-function-concurrency", "--function-name", "send-email"]);
});

test("lambda__function-config and lambda__config-edit: the general settings, and the form that changes them", async ({ page }) => {
  await go(page, fn("send-email", "?tab=configuration"), "send-email");
  await expect(page.getByTestId("config-nav").getByRole("button")).toHaveText(["General configuration", "Triggers", "Permissions", "Function URL", "Environment variables"]);
  const card = page.getByTestId("general-card");
  await expect(card).toContainText("Send alert and invoice emails");
  await expect(card).toContainText("256 MB");
  await expect(card).toContainText("512 MB");
  await expect(card).toContainText("30 seconds");
  await expect(card).toContainText("lambda-exec");
  await expect(card).toContainText(`arn:aws:iam::${server.account}:role/lambda-exec`);
  await expect(page.getByTestId("general-note")).toContainText("Stored, not applied locally");
  await expect(page.getByTestId("recent-calls")).toBeVisible();
  await shot(page, "lambda__function-config");
  await page.getByTestId("config-edit").click();
  await expect(page.locator("h1").first()).toHaveText("Edit General configuration");
  await expect(page.getByTestId("cfg-description")).toHaveValue("Send alert and invoice emails");
  await page.getByTestId("cfg-memory").fill("512");
  await expect(code(page)).toContainText('"MemorySize": 512');
  await expect(code(page)).toContainText('"FunctionName": "send-email"');
  await expect(code(page)).not.toContainText("Timeout");
  await expect(page.getByTestId("field-memory")).toContainText("Changed from 256 MB. Stored, not applied locally.");
  await expect(page.getByTestId("field-timeout")).toContainText("The SQS queue email-outbox must hide a message for at least as long as this.");
  await expect(page.getByTestId("field-role")).toContainText("dispatch-hook, resize-product-image and weekly-digest use lambda-exec too.");
  await expect(page.getByTestId("field-ephemeral")).toContainText("Stored, not applied locally");
  await shot(page, "lambda__config-edit");
  // A value out of range is said and not sent.
  await page.getByTestId("cfg-memory").fill("64");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("field-memory")).toContainText("Memory is a whole number from 128 to 10,240 MB.");
  await page.getByTestId("form-cancel").click();
  await expect(page.getByTestId("general-card")).toContainText("256 MB");
  // Saved on a function nothing else is drawn from: the change is made, and the page says so.
  await go(page, fn("dispatch-hook", "?tab=configuration&p=general&view=config-edit"), "Edit General configuration");
  await page.getByTestId("cfg-memory").fill("192");
  await page.getByTestId("cfg-timeout").fill("12");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("The configuration of dispatch-hook was saved.");
  await expect(page.getByTestId("general-card")).toContainText("192 MB");
  await expect(page.getByTestId("general-card")).toContainText("12 seconds");
});

test("lambda__function-env, lambda__env-edit and lambda__env-none: the variables, the form with its rules, and a function with none", async ({ page }) => {
  await go(page, fn("send-email", "?tab=configuration&p=env"), "send-email");
  await expect(page.getByTestId("env-key")).toHaveText(["LOG_LEVEL", "MAIL_FROM", "SMTP_HOST"]);
  await expect(page.getByTestId("env-value")).toHaveText(["INFO", "alerts@larder.dev", "smtp.larder.dev"]);
  await expect(page.getByTestId("env-teach")).toContainText("locked when you publish");
  await expect(page.getByTestId("env-teach")).toContainText("Keep secrets in Secrets Manager, such as larder/prod/stripe-key, rather than here.");
  await shot(page, "lambda__function-env");
  await page.getByTestId("env-edit").click();
  await expect(page.locator("h1").first()).toHaveText("Edit environment variables");
  await page.getByRole("button", { name: "Add environment variable" }).click();
  await page.getByTestId("env-kv-key-3").fill("RETRY_LIMIT");
  await page.getByTestId("env-kv-value-3").fill("3");
  await expect(code(page)).toContainText('"RETRY_LIMIT": "3"');
  await expect(code(page)).toContainText('"LOG_LEVEL": "INFO"');
  await expect(page.getByTestId("env-rules")).toContainText("RETRY_LIMIT is the new variable.");
  await expect(page.getByTestId("env-rules")).toContainText("AWS_REGION and _HANDLER, are refused");
  await shot(page, "lambda__env-edit");
  // A key Lambda reserves is said before it is sent.
  await page.getByTestId("env-kv-key-3").fill("AWS_REGION");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("env-problem-3")).toContainText("AWS_REGION is a variable Lambda sets itself");
  await page.getByTestId("form-cancel").click();
  await go(page, fn("dispatch-hook", "?tab=configuration&p=env"), "dispatch-hook");
  await expect(page.getByTestId("env-none")).toContainText("dispatch-hook has no environment variables.");
  await shot(page, "lambda__env-none");
  await page.getByTestId("env-edit").click();
  await page.getByRole("button", { name: "Add environment variable" }).click();
  await page.getByTestId("env-kv-key-0").fill("GREETING");
  await page.getByTestId("env-kv-value-0").fill("hello");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("The environment variables of dispatch-hook were saved.");
  await expect(page.getByTestId("env-key")).toHaveText(["GREETING"]);
});

test("lambda__function-triggers, lambda__trigger-edit and lambda__triggers-none: the mappings Lambda owns, and a function with none", async ({ page }) => {
  await go(page, fn("send-email", "?tab=configuration&p=triggers"), "send-email");
  const row = page.getByRole("table", { name: "Triggers" }).locator("tr[data-row]");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("email-outbox");
  await expect(row).toContainText(`arn:aws:sqs:us-east-1:${server.account}:email-outbox`);
  await expect(row).toContainText("Enabled");
  await expect(row).toContainText("Amazon SQS event source mapping, batch size 10");
  await expect(row).toContainText("UUID ");
  await expect(page.getByTestId("triggers-teach")).toContainText("The alerts topic is such a trigger: its statement is on the Permissions page.");
  await expect(page.getByTestId("trigger-edit")).toBeVisible();
  await expect(page.getByTestId("trigger-add")).toBeVisible();
  await row.click();
  await shot(page, "lambda__function-triggers");
  await page.getByTestId("trigger-edit").click();
  await expect(page.locator("h1").first()).toHaveText("Edit trigger");
  await expect(page.getByTestId("trigger-queue")).toHaveValue(`arn:aws:sqs:us-east-1:${server.account}:email-outbox`);
  await expect(page.getByTestId("field-queue")).toContainText("An existing mapping cannot change its source: UpdateEventSourceMapping takes no event source.");
  await expect(page.getByTestId("field-queue")).toContainText("email-outbox holds 0 messages.");
  await page.getByTestId("trigger-batch").fill("5");
  await expect(code(page)).toContainText('"BatchSize": 5');
  await expect(code(page)).toContainText('"UUID"');
  await expect(page.getByTestId("field-batch")).toContainText("Changed from 10.");
  await shot(page, "lambda__trigger-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("Trigger saved.");
  await expect(page.getByRole("table", { name: "Triggers" })).toContainText("batch size 5");
  cli(["lambda", "update-event-source-mapping", "--uuid", cli(["lambda", "list-event-source-mappings", "--function-name", "send-email", "--query", "EventSourceMappings[0].UUID", "--output", "text"]).trim(), "--batch-size", "10"]);
  await go(page, fn("dispatch-hook", "?tab=configuration&p=triggers"), "dispatch-hook");
  await expect(page.getByTestId("triggers-none")).toContainText("Nothing invokes dispatch-hook automatically. Add a trigger such as a queue, a stream or a schedule.");
  await expect(page.getByTestId("triggers-none").getByRole("button", { name: "Add trigger" })).toBeVisible();
  await shot(page, "lambda__triggers-none");
});

test("lambda__trigger-add: an SQS mapping, with the visibility timeout it needs", async ({ page }) => {
  await go(page, fn("send-email", "?tab=configuration&p=triggers"), "send-email");
  await page.getByTestId("trigger-add").click();
  await expect(page.locator("h1").first()).toHaveText("Add trigger");
  await expect(page.getByTestId("trigger-type")).toContainText("SQS");
  await choose(page, "Choose a queue", "invoice-queue");
  await expect(page.getByTestId("queue-hint")).toContainText("invoice-queue is a standard queue that the SNS topic orders-created delivers to.");
  await expect(code(page)).toContainText(`"EventSourceArn": "arn:aws:sqs:us-east-1:${server.account}:invoice-queue"`);
  await expect(code(page)).toContainText('"FunctionName": "send-email"');
  await expect(code(page)).toContainText('"Enabled": true');
  await expect(code(page)).toContainText('"BatchSize": 10');
  await expect(page.getByTestId("visibility-note")).toContainText("at least six times the function's timeout (180 seconds for the 30 second timeout of send-email)");
  await expect(page.getByTestId("visibility-note")).toContainText("invoice-queue hides a message for 30 seconds, less than the 180 seconds that six times the timeout asks.");
  await expect(page.getByTestId("section-more")).toContainText("Batch window, partial batch responses, maximum concurrency and filter criteria");
  await page.getByTestId("section-more-toggle").click();
  await page.getByTestId("map-window").fill("5");
  await page.getByTestId("map-concurrency").fill("4");
  await expect(code(page)).toContainText('"MaximumBatchingWindowInSeconds": 5');
  await expect(code(page)).toContainText('"MaximumConcurrency": 4');
  await page.getByTestId("map-window").fill("");
  await page.getByTestId("map-concurrency").fill("");
  await page.getByTestId("section-more-toggle").click();
  await shot(page, "lambda__trigger-add");
  await page.getByTestId("form-cancel").click();
  // Added for real on the function that has no trigger (its timeout is within the queue's visibility timeout).
  await go(page, fn("dispatch-hook", "?tab=configuration&p=triggers&view=trigger-add"), "Add trigger");
  await choose(page, "Choose a queue", "analytics-queue");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("Trigger added.");
  await expect(page.getByRole("table", { name: "Triggers" })).toContainText("analytics-queue");
  // A mapping the queue refuses: a function that outlasts its visibility timeout.
  cli(["lambda", "update-function-configuration", "--function-name", "dispatch-hook", "--timeout", "100"]);
  await go(page, fn("dispatch-hook", "?tab=configuration&p=triggers&view=trigger-add"), "Add trigger");
  await choose(page, "Choose a queue", "invoice-queue");
  await expect(page.getByTestId("visibility-note")).toContainText("which is shorter than the timeout: Lambda refuses the mapping.");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("call-panel-came-back")).toBeVisible();
  cli(["lambda", "update-function-configuration", "--function-name", "dispatch-hook", "--timeout", "12"]);
});

test("lambda__destination-add: where the asynchronous invocations report, and the queue that cannot be one", async ({ page }) => {
  await go(page, fn("send-email"), "send-email");
  await page.getByTestId("verb-destination").click();
  await expect(page.locator("h1").first()).toHaveText("Add destination");
  await expect(page.getByTestId("field-source")).toContainText("Asynchronous invocation");
  await expect(page.getByTestId("field-source")).toContainText("The alerts topic invokes send-email this way.");
  await expect(page.getByTestId("dtype-off")).toHaveCount(2);
  await choose(page, "Choose a queue", "orders-events.fifo");
  await expect(page.getByTestId("field-target")).toContainText("orders-events.fifo is a FIFO queue, so it cannot be a destination.");
  await expect(page.getByTestId("field-target")).toContainText("Only standard queues and topics work.");
  await choose(page, "Choose a queue", "analytics-queue");
  await expect(code(page)).toContainText('"OnFailure"');
  await expect(code(page)).toContainText(`arn:aws:sqs:us-east-1:${server.account}:analytics-queue`);
  await expect(page.getByTestId("call-panel-code")).toContainText("UpdateFunctionEventInvokeConfig");
  await expect(page.getByTestId("call-panel").getByText("Update changes only the member you send. Put would replace the whole async configuration.")).toBeVisible();
  await expect(page.getByTestId("field-target")).toContainText("sqs:SendMessage");
  await shot(page, "lambda__destination-add");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("Destination saved.");
  expect(cli(["lambda", "get-function-event-invoke-config", "--function-name", "send-email"])).toContain("analytics-queue");
});

test("lambda__function-permissions, lambda__permission-add and lambda__permissions-none: who may call the function", async ({ page }) => {
  await go(page, fn("send-email", "?tab=configuration&p=permissions"), "send-email");
  await expect(page.getByTestId("role-card")).toContainText("lambda-exec");
  await expect(page.getByTestId("role-card")).toContainText(`arn:aws:iam::${server.account}:role/lambda-exec`);
  const row = page.getByRole("table", { name: "Resource-based policy statements" }).locator("tr[data-row]");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("sns-alerts");
  await expect(row).toContainText("Allow");
  await expect(row).toContainText("sns.amazonaws.com");
  await expect(row).toContainText("lambda:InvokeFunction");
  await expect(row).toContainText("ArnLike aws:SourceArn arn:aws:sns:us-east-1");
  await expect(page.getByTestId("policy-teach")).toContainText("aws:PrincipalOrgID");
  await shot(page, "lambda__function-permissions");
  await page.getByTestId("permission-add").click();
  await expect(page.locator("h1").first()).toHaveText("Add permissions");
  await page.getByTestId("perm-sid").fill("sns-alerts");
  await expect(page.getByTestId("field-sid")).toContainText("sns-alerts is taken");
  await page.getByTestId("perm-sid").fill("s3-receipts");
  await page.getByTestId("perm-principal").fill("s3.amazonaws.com");
  await page.getByTestId("perm-source-arn").fill("arn:aws:s3:::receipts");
  await page.getByTestId("perm-source-account").fill(server.account);
  await expect(code(page)).toContainText('"StatementId": "s3-receipts"');
  await expect(code(page)).toContainText('"Action": "lambda:InvokeFunction"');
  await expect(code(page)).toContainText('"Principal": "s3.amazonaws.com"');
  await expect(code(page)).toContainText('"SourceArn": "arn:aws:s3:::receipts"');
  await expect(code(page)).toContainText(`"SourceAccount": "${server.account}"`);
  await expect(page.getByTestId("field-source-arn")).toContainText("for S3, a bucket such as arn:aws:s3:::receipts");
  await shot(page, "lambda__permission-add");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("Permission added.");
  await expect(page.getByRole("table", { name: "Resource-based policy statements" })).toContainText("s3-receipts");
  await go(page, fn("dispatch-hook", "?tab=configuration&p=permissions"), "dispatch-hook");
  await expect(page.getByTestId("policy-none")).toContainText("No other service or account has been given permission to invoke dispatch-hook.");
  await expect(page.getByTestId("policy-none").getByRole("button", { name: "Add permissions" })).toBeVisible();
  await shot(page, "lambda__permissions-none");
});

test("lambda__function-url, lambda__url-create and lambda__function-url-created: a function with no URL, the form, and the URL", async ({ page }) => {
  await go(page, fn("send-email", "?tab=configuration&p=url"), "send-email");
  await expect(page.getByTestId("url-none")).toContainText("send-email has no URL. A URL can be added to $LATEST or to an alias, but not to another version.");
  await shot(page, "lambda__function-url");
  await page.getByTestId("url-create").click();
  await expect(page.locator("h1").first()).toHaveText("Create function URL");
  await expect(page.getByTestId("auth-AWS_IAM")).toBeChecked();
  await page.getByTestId("url-cors").click();
  await page.getByTestId("url-origins").fill("https://admin.larder.dev");
  await page.getByTestId("url-methods").fill("POST");
  await page.getByTestId("url-headers").fill("content-type");
  await page.getByTestId("url-max-age").fill("5");
  await expect(code(page)).toContainText('"AuthType": "AWS_IAM"');
  await expect(code(page)).toContainText('"AllowOrigins": [');
  await expect(code(page)).toContainText('"https://admin.larder.dev"');
  await expect(code(page)).toContainText('"MaxAge": 5');
  await expect(page.getByTestId("call-panel").getByText("No Qualifier, so the URL is for $LATEST.")).toBeVisible();
  await shot(page, "lambda__url-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("A function URL was created for the $LATEST unpublished version.");
  await expect(page.getByTestId("header-url")).toContainText(".lambda-url.us-east-1.localhost");
  const card = page.getByTestId("url-card");
  await expect(card).toContainText("AWS_IAM");
  await expect(card).toContainText("$LATEST, the unpublished version");
  await expect(card).toContainText("https://admin.larder.dev");
  await expect(card).toContainText("5 seconds");
  await expect(card).toContainText(`${server.endpoint}/_aws/lambda-url/us-east-1/`);
  await expect(page.getByTestId("url-teach")).toContainText("Once you create a function URL, its URL endpoint never changes.");
  await shot(page, "lambda__function-url-created");
  // The "also at" address answers, signed or not: it is how a client that cannot set the Host header reaches it.
  const also = (await card.getByText(`${server.endpoint}/_aws/lambda-url/us-east-1/`).first().textContent()) ?? "";
  const res = await fetch(also.trim());
  expect([200, 403, 502]).toContain(res.status);
});

test("lambda__function-aliases, lambda__alias-create and lambda__aliases-none: the aliases, the form, and a function with none", async ({ page }) => {
  await go(page, fn("send-email", "?tab=aliases"), "send-email");
  const rows = page.getByRole("table", { name: "Aliases" }).locator("tbody tr[data-row]");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText("live");
  await expect(rows.nth(0)).toContainText("Production traffic");
  await expect(rows.nth(0)).toContainText(`arn:aws:lambda:us-east-1:${server.account}:function:send-email:live`);
  await expect(rows.nth(1)).toContainText("staging");
  await expect(page.getByTestId("lambda-aliases")).toContainText("An alias is a pointer to a function version that you can update. Callers use the alias ARN and never change.");
  await expect(page.getByTestId("lambda-aliases")).toContainText("The columns are fields of ListAliases: Name, Description, FunctionVersion and AliasArn.");
  await shot(page, "lambda__function-aliases");
  await page.getByTestId("alias-create").click();
  await expect(page.locator("h1").first()).toHaveText("Create alias");
  await page.getByTestId("alias-name").fill("live");
  await expect(page.getByTestId("field-name")).toContainText("live and staging already exist.");
  await expect(page.getByTestId("field-name")).toContainText("live already exists");
  await page.getByTestId("alias-name").fill("smoke");
  await page.getByTestId("alias-description").fill("Smoke tests on version 2");
  await choose(page, "", "2");
  await expect(code(page)).toContainText('"Name": "smoke"');
  await expect(code(page)).toContainText('"FunctionVersion": "2"');
  await expect(code(page)).toContainText('"Description": "Smoke tests on version 2"');
  await expect(page.getByTestId("field-version")).toContainText("send-email has versions 1, 2, 3; live points at 2, staging points at 3.");
  await expect(page.getByTestId("section-weighted")).toContainText("Weighted alias");
  await shot(page, "lambda__alias-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("Alias created.");
  await expect(page.getByRole("table", { name: "Aliases" }).locator("tbody tr[data-row]")).toHaveCount(3);
  await go(page, fn("dispatch-hook", "?tab=aliases"), "dispatch-hook");
  await expect(page.getByTestId("aliases-none")).toContainText("dispatch-hook has no aliases. An alias is a pointer to a version that you can move without changing callers.");
  await expect(page.getByTestId("aliases-none").getByRole("button", { name: "Create alias" })).toBeVisible();
  await shot(page, "lambda__aliases-none");
});

test("lambda__function-versions and lambda__version-publish: the versions, and the dialog that publishes the next", async ({ page }) => {
  await go(page, fn("send-email", "?tab=versions"), "send-email");
  const rows = page.getByRole("table", { name: "Versions" }).locator("tbody tr[data-row]");
  await expect(rows).toHaveCount(4);
  await expect(rows.nth(0)).toContainText("$LATEST");
  await expect(rows.nth(0)).toContainText("Send alert and invoice emails");
  await expect(rows.nth(1)).toContainText("html alert template");
  await expect(rows.nth(3)).toContainText("first deploy");
  await expect(rows.nth(1)).toContainText(`arn:aws:lambda:us-east-1:${server.account}:function:send-email:3`);
  await expect(page.getByTestId("lambda-versions")).toContainText("A version is an immutable copy of the code and settings. Numbers only go up and are never reused");
  await expect(page.getByTestId("lambda-versions")).toContainText("The columns are fields of ListVersionsByFunction: Version, Description, LastModified and FunctionArn.");
  await expect(page.getByTestId("version-publish")).toHaveText("Publish new version");
  await shot(page, "lambda__function-versions");
  await page.getByTestId("version-publish").click();
  const dialog = page.getByTestId("publish-dialog");
  await expect(dialog).toContainText("Lambda creates version 4 from the current code and settings of $LATEST.");
  await expect(dialog).toContainText("changed after version 3");
  await expect(dialog).toContainText("Once published, the code and most settings cannot be changed.");
  await page.getByTestId("version-description").fill("log the request id");
  await shot(page, "lambda__version-publish");
  await page.getByTestId("publish-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("Version published.");
  await expect(page.getByRole("table", { name: "Versions" }).locator("tbody tr[data-row]")).toHaveCount(5);
  await expect(page.getByRole("table", { name: "Versions" })).toContainText("log the request id");
  // Nothing changed since: Lambda would answer with version 4, and the dialog says so.
  await page.getByTestId("version-publish").click();
  await expect(page.getByTestId("publish-dialog")).toContainText("Nothing changed since version 4");
  await page.keyboard.press("Escape");
});

test("lambda__function-code: an edit is deployed with the files it did not touch", async ({ page }) => {
  await go(page, fn("send-email"), "send-email");
  const editor = page.getByTestId("code-editor");
  await editor.fill(`${await editor.inputValue()}# deployed from the console\n`);
  await expect(page.getByTestId("code-state")).toHaveText("1 file changed, not deployed.");
  await expect(page.getByTestId("code-explorer")).toContainText("edited");
  const before = cli(["lambda", "get-function-configuration", "--function-name", "send-email", "--query", "CodeSha256", "--output", "text"]);
  await page.getByTestId("code-deploy").click();
  await expect(page.getByTestId("done-banner")).toContainText("The code of send-email was deployed.");
  await expect(page.getByTestId("code-editor")).toHaveValue(/# deployed from the console/);
  expect(cli(["lambda", "get-function-configuration", "--function-name", "send-email", "--query", "CodeSha256", "--output", "text"])).not.toBe(before);
  // It still runs: the archive the page wrote is one Lambda unpacks.
  const out = invoke(server.endpoint, "send-email", { Records: [{ Sns: { Subject: "after-deploy" } }] });
  expect(out).toContain('"StatusCode": 200');
  expect(out).not.toContain("FunctionError");
});

test("lambda__layers-list: the account's layers and whether a function uses one", async ({ page }) => {
  await go(page, "/resources/lambda/layers", "Layers");
  await expect(page.getByTestId("list-sentence")).toHaveText("1 layer here, larder-shared, with 3 versions. No function uses it yet.");
  const row = page.getByRole("table", { name: "Layers" }).locator("tr[data-row]");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("larder-shared");
  await expect(row).toContainText("Logging and mail helpers");
  await expect(row).toContainText("python3.13, nodejs22.x");
  await expect(row).toContainText("x86_64");
  await expect(page.getByTestId("create-open")).toHaveText("Create layer");
  await expect(page.getByTestId("page-line")).toHaveText("1 to 1 of 1 layer");
  await shot(page, "lambda__layers-list");
});

test("lambda__layer-add: a layer of this account and one of its versions, set on the function", async ({ page }) => {
  await go(page, fn("send-email"), "send-email");
  await page.getByTestId("layer-add").click();
  await expect(page.locator("h1").first()).toHaveText("Add a layer");
  await expect(page.getByTestId("layer-aws-off")).toContainText("Not available in doze-aws: AWS-managed extension layers are hosted by AWS.");
  await expect(page.getByTestId("field-layer")).toContainText("larder-shared is the only layer in this account.");
  await expect(page.getByTestId("layer-version")).toContainText("3");
  await expect(page.getByTestId("field-version")).toContainText("Version 3 is the newest of larder-shared.");
  await expect(code(page)).toContainText(`"arn:aws:lambda:us-east-1:${server.account}:layer:larder-shared:3"`);
  await expect(code(page)).toContainText('"Layers": [');
  await expect(page.getByTestId("layer-limits")).toContainText("up to five layers");
  await shot(page, "lambda__layer-add");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("Layer added.");
  await expect(page.getByTestId("layers-list")).toContainText("larder-shared");
  await expect(page.getByTestId("layers-list")).toContainText("version 3");
  // A function that uses a layer says so in the list of layers.
  await go(page, "/resources/lambda/layers", "Layers");
  await expect(page.getByTestId("list-sentence")).toContainText("1 function uses it.");
});

test("lambda__layer-create: a name, a .zip, its compatibility, and the call", async ({ page }) => {
  await go(page, "/resources/lambda/layers?create=yes", "Create layer");
  await page.getByTestId("layer-name").fill("larder-shared");
  await expect(page.getByTestId("field-name")).toContainText("Publishing under the name larder-shared again would make version 4.");
  await page.getByTestId("layer-name").fill("mail-templates");
  await page.getByTestId("layer-description").fill("HTML and text templates for outgoing mail");
  await page.getByTestId("layer-file").setInputFiles(zipOf("mail.py", "# templates\n"));
  await expect(page.getByTestId("layer-file-name")).toContainText("fn.zip");
  await page.getByTestId("layer-arch-x86_64").click();
  await page.getByLabel("Compatible runtimes", { exact: true }).click();
  await page.getByRole("option", { name: "python3.13" }).click();
  await page.getByRole("option", { name: "nodejs22.x" }).click();
  await page.keyboard.press("Escape");
  await page.getByTestId("layer-license").fill("MIT");
  await expect(code(page)).toContainText('"LayerName": "mail-templates"');
  await expect(code(page)).toContainText('"CompatibleArchitectures": [');
  await expect(code(page)).toContainText('"python3.13"');
  await expect(code(page)).toContainText('"LicenseInfo": "MIT"');
  await expect(code(page)).toContainText("<fn.zip>");
  await shot(page, "lambda__layer-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("published-banner")).toContainText("mail-templates version 1 was published.");
  expect(cli(["lambda", "list-layers", "--query", "Layers[].LayerName", "--output", "text"])).toContain("mail-templates");
});

test("lambda__layers-none: the empty list once every layer version is deleted", async ({ page }) => {
  // The last version of a layer is deleted from its own page, and the list it lands on says the layer is gone.
  await go(page, "/resources/lambda/layers/mail-templates", "mail-templates");
  await page.getByTestId("layer-version-delete").click();
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page.getByTestId("deleted-banner")).toContainText("mail-templates was deleted.");
  for (const v of ["1", "2", "3"]) cli(["lambda", "delete-layer-version", "--layer-name", "larder-shared", "--version-number", v]);
  await go(page, "/resources/lambda/layers", "Layers");
  await expect(page.getByTestId("list-empty")).toContainText("No layers");
  await expect(page.getByTestId("list-empty")).toContainText("A layer packages libraries or data that functions share.");
  await expect(page.getByTestId("list-empty").getByRole("button", { name: "Create layer" })).toBeVisible();
  await shot(page, "lambda__layers-none");
});

test("lambda__function-deleted and lambda__functions-none: the list after a delete, what it left behind, and the empty list", async ({ page }) => {
  await go(page, fn("send-email"), "send-email");
  await page.getByTestId("verb-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("its 4 versions and its 3 aliases (live, smoke, staging)");
  await page.getByTestId("delete-dialog-input").fill("confirm");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("send-email was deleted.");
  await expect(page.locator("h1").first()).toHaveText("Event Functions");
  await expect(page.getByTestId("list-sentence")).toContainText("3 functions run here: dispatch-hook, export-report, resize-product-image, weekly-digest.".replace("3 functions", "4 functions"));
  await expect(page.getByTestId("list-sentence")).toContainText("The alerts topic still points at the deleted function; remove that subscription in SNS.");
  await expect(page.getByTestId("left-behind")).toContainText("Event source mappings are separate resources: the mapping for email-outbox");
  await expect(page.getByTestId("left-behind")).toContainText("still exists. Delete it with DeleteEventSourceMapping.");
  await expect(page.getByRole("table", { name: "Functions" }).locator("tbody tr[data-row]")).toHaveCount(4);
  const mapping = cli(["lambda", "list-event-source-mappings", "--query", "EventSourceMappings[].UUID", "--output", "text"]);
  expect(mapping.trim().length).toBeGreaterThan(0);
  await shot(page, "lambda__function-deleted");
  for (const name of ["dispatch-hook", "export-report", "resize-product-image", "weekly-digest"]) cli(["lambda", "delete-function", "--function-name", name]);
  await go(page, "/resources/lambda/functions", "Event Functions");
  await expect(page.getByTestId("list-empty")).toContainText("No functions");
  await expect(page.getByTestId("list-empty")).toContainText("You have no functions in this region. Create a function to run code without managing servers.");
  await expect(page.getByTestId("list-empty").getByRole("button", { name: "Create function" })).toBeVisible();
  await expect(page.getByTestId("create-open")).toHaveText("Create Event Function");
  await shot(page, "lambda__functions-none");
});
