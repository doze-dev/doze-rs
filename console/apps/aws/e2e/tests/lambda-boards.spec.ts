// The 37 Lambda boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder" world
// with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { start, type Running, aws } from "../server";
import { CUT, expect, test } from "./fixtures";
import { invoke, seedLarder } from "./lambda-seed";

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
  await expect(page.getByTestId("list-sentence")).toContainText("send-email was invoked 3 times in the last hour");
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
  await expect(page.getByTestId("invoke-status")).toHaveText("Status: Succeeded");
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
  await page.getByTestId("invoke-event").fill("{}");
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
