// The Home and Connect boards, each reached against a real doze-aws (one per browser, its apps called as Go, Python and the AWS
// CLI) and, in Chromium, photographed light and dark into SHOTS_DIR to be set beside the board.
import { expect, test } from "./fixtures";
import { as } from "./home-seed";
import { cli, go, serveWorld, shot, world } from "./home-kit";

// `--idle-secs 4`: a service no call has reached for four seconds is asleep, so the board's Awake and Asleep both appear.
serveWorld(0, ["--idle-secs", "4"]);

test("home: what is awake by group, who is connected, what needs a look, and the newest calls", async ({ page }) => {
  // The apps called every service a few seconds ago; keep SQS and SNS busy so they are awake and the rest are asleep.
  await new Promise((r) => setTimeout(r, 4_500));
  as(world.server.endpoint, world.apps.payments, ["sqs", "list-queues"]);
  as(world.server.endpoint, world.apps.payments, ["sns", "list-topics"]);
  await go(page, "/", "Home");
  await expect(page.getByTestId("card-connect")).toContainText("Where your app connects");
  await expect(page.getByTestId("endpoint")).toHaveText(world.server.endpoint);
  // Heard from the newest app, by its name and client.
  await expect(page.getByTestId("connected-heard")).toContainText("Heard from payments (AWS CLI)");
  // Seven groups, each with its services; SQS and SNS awake, the rest asleep.
  await expect(page.getByTestId("awake-group")).toHaveCount(7);
  const group = (name: string) => page.locator(`[data-testid="awake-group"][data-group="${name}"]`);
  await expect(group("Messaging and events").locator('[data-service="sqs"]')).toHaveAttribute("data-state", "awake");
  await expect(group("Messaging and events").locator('[data-service="sns"]')).toHaveAttribute("data-state", "awake");
  await expect(group("Storage and data").locator('[data-service="s3"]')).toHaveAttribute("data-state", "asleep");
  await expect(group("Identity and access")).toContainText("STS");
  await expect(group("Compute and flow").locator('[data-service="stepfunctions"] [data-testid="awake-last"]')).toHaveText("Not called yet");
  // Needs a look: soft IAM, a call not built, a dead-letter queue, a subscription stored.
  const signals = page.getByTestId("needs-a-look");
  await expect(signals).toContainText("would be denied");
  await expect(signals).toContainText("NOT BUILT");
  await expect(signals).toContainText("SelectObjectContent");
  await expect(signals).toContainText("1 message in a dead-letter queue");
  await expect(signals).toContainText("Not active locally");
  await expect(signals).toContainText("1 subscription stored");
  // Recent activity names the resource and the app, not the URL and the key.
  const recent = page.getByRole("table", { name: "Recent calls" });
  await expect(recent).toContainText("admin-cli");
  await expect(recent).toContainText("receipts/r-8841.pdf");
  await expect(recent).not.toContainText("http://127.0.0.1");
  await expect(recent).toContainText("catalog");
  await shot(page, "home");
});

test("home__home-services: where the list of services is set, and what this run left out", async ({ page }) => {
  await go(page, "/", "Home");
  await page.getByTestId("which-services").click();
  const sheet = page.getByTestId("services-sheet");
  await expect(sheet).toBeVisible();
  await expect(sheet.getByTestId("services-sheet-lede")).toContainText("All 18 services run by default.");
  await expect(sheet.getByTestId("services-toml")).toContainText('services = ["sqs", "sns", "eventbridge"');
  await expect(sheet.getByTestId("services-flag")).toContainText("doze-aws --services sqs,sns");
  await expect(sheet).toContainText("Takes a restart.");
  await expect(sheet).toContainText("Unknown service names are an error.");
  await shot(page, "home__home-services");
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
});

test("home__connect: one address, the environment, the settings and URLs of an IP, and the last call that reached it", async ({ page }) => {
  await go(page, "/connect", "Connect your app");
  await expect(page.getByTestId("connect-endpoint")).toHaveText(world.server.endpoint);
  await expect(page.getByTestId("env-block")).toContainText(`export AWS_ENDPOINT_URL=${world.server.endpoint}`);
  await expect(page.getByTestId("env-block")).toContainText("export AWS_SECRET_ACCESS_KEY=test");
  await expect(page.getByTestId("connect-keys")).toContainText("Credentials are not checked.");
  await expect(page.getByTestId("connect-env-cmd")).toContainText("eval");
  await expect(page.getByTestId("setting-s3")).toContainText('Config(s3={"addressing_style": "path"})');
  await expect(page.getByTestId("setting-sfn")).toContainText("disableHostPrefix: true");
  const shapes = page.getByTestId("url-shape");
  await expect(shapes).toHaveCount(4);
  await expect(shapes.nth(0)).toContainText(`${world.server.endpoint}/${world.server.account}/pick-lists`);
  await expect(shapes.nth(3)).toContainText("/_aws/lambda-url/us-east-1/<id>/");
  const check = page.getByTestId("connect-heard");
  await expect(check).toContainText("IP and port");
  await expect(check).toContainText("payments");
  await expect(check).toContainText("ListTopics");
  await expect(page.getByTestId("connect-follows")).toContainText("Nothing stored changes when you switch.");
  await shot(page, "home__connect");
  // The same page is the standalone board: one address, no name.
  await shot(page, "home__connect-standalone");
});

test("home__connect-waiting: Check again starts over from now, with a call to send", async ({ page }) => {
  await go(page, "/connect", "Connect your app");
  await expect(page.getByTestId("connect-heard")).toBeVisible();
  await page.getByTestId("check-again").click();
  const waiting = page.getByTestId("connect-waiting");
  await expect(waiting).toBeVisible();
  await expect(waiting).toContainText("Waiting for a call");
  await expect(page.getByTestId("connect-probe")).toContainText(`aws sts get-caller-identity \\`);
  await expect(page.getByTestId("connect-probe")).toContainText(`--endpoint-url ${world.server.endpoint}`);
  await expect(waiting).toContainText("only counts calls made after it");
  await shot(page, "home__connect-waiting");
  // Sending the call the page shows ticks the row, from this one on.
  cli(["sts", "get-caller-identity"]);
  await expect(page.getByTestId("connect-heard")).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId("connect-heard")).toContainText("GetCallerIdentity");
  await expect(page.getByTestId("connect-waiting")).toHaveCount(0);
});

test("home__connect-code: the same call from each client, boto3 first, with what S3 needs", async ({ page }) => {
  await go(page, "/connect", "Connect your app");
  await page.getByTestId("tab-code").click();
  await expect(page.getByTestId("code-lede")).toContainText("any non-empty values work");
  const ids = await page.locator('[data-testid^="snippet-"]').evaluateAll((els) => els.map((e) => e.getAttribute("data-testid")));
  expect(ids.slice(0, 4)).toEqual(["snippet-boto3", "snippet-rust", "snippet-js", "snippet-cli"]);
  await expect(page.getByTestId("snippet-boto3")).toContainText(`endpoint_url="${world.server.endpoint}"`);
  await expect(page.getByTestId("snippet-rust")).toContainText("cargo add aws-config aws-sdk-sqs");
  await expect(page.getByTestId("snippet-rust")).toContainText("use aws_config::{BehaviorVersion, Region};");
  await expect(page.getByTestId("snippet-cli")).toContainText(`aws sqs list-queues --endpoint-url ${world.server.endpoint}`);
  await expect(page.getByTestId("connect-s3")).toContainText("Using S3 or Step Functions sync calls on an IP and port?");
  await shot(page, "home__connect-code");
});

test("home__connect-services: where each service answers, its variable, and what its URLs are", async ({ page }) => {
  await go(page, "/connect", "Connect your app");
  await page.getByTestId("tab-services").click();
  const table = page.getByRole("table", { name: "Where each service answers" });
  await expect(table.locator("tbody tr[data-row]")).toHaveCount(18);
  const row = (name: string) => table.locator(`tr[data-row="${name}"]`);
  await expect(row("sqs")).toContainText("AWS_ENDPOINT_URL_SQS");
  await expect(row("sqs")).toContainText(`Queue URLs: /${world.server.account}/<queue>`);
  await expect(row("s3")).toContainText("Path style: /<bucket>/<key>");
  await expect(row("lambda")).toContainText("Function URLs: /_aws/lambda-url/us-east-1/<id>/");
  await expect(row("stepfunctions")).toContainText("AWS_ENDPOINT_URL_SFN");
  await expect(row("stepfunctions")).toContainText("Sync calls need disableHostPrefix");
  await expect(row("logs")).toContainText("AWS_ENDPOINT_URL_CLOUDWATCH_LOGS");
  await expect(row("apigateway")).toContainText("Deployed APIs: /_aws/execute-api/<id>/<stage>/");
  await shot(page, "home__connect-services");
});

test("home__connect-urls: invoke URLs and sync calls", async ({ page }) => {
  await go(page, "/connect", "Connect your app");
  await page.getByTestId("tab-services").click();
  await page.getByTestId("more-lambda").click();
  const sheet = page.getByTestId("invoke-sheet");
  await expect(sheet).toBeVisible();
  await expect(sheet.getByTestId("invoke-api")).toContainText(`${world.server.endpoint}/_aws/execute-api/<id>/<stage>/`);
  await expect(sheet.getByTestId("invoke-lambda")).toContainText(`${world.server.endpoint}/_aws/lambda-url/us-east-1/<id>/`);
  await expect(sheet.getByTestId("invoke-sync")).toContainText("disableHostPrefix: true");
  await expect(sheet.getByTestId("invoke-sync")).toContainText("Config(inject_host_prefix=False)");
  await shot(page, "home__connect-urls");
});

test("home__connect-s3: virtual-hosted and path style, and the setting for each client", async ({ page }) => {
  await go(page, "/connect", "Connect your app");
  await page.getByTestId("tab-services").click();
  await page.getByTestId("more-s3").click();
  const sheet = page.getByTestId("s3-sheet");
  await expect(sheet).toBeVisible();
  await expect(sheet.getByTestId("s3-virtual")).toContainText("product-images.s3.us-east-1.localhost");
  await expect(sheet.getByTestId("s3-path")).toContainText(`${world.server.endpoint}/product-images/sku-FV-TOM-V.jpg`);
  await expect(sheet.getByTestId("s3-path")).toContainText(".force_path_style(true)");
  await expect(sheet.getByTestId("s3-path")).toContainText("aws configure set default.s3.addressing_style path");
  await shot(page, "home__connect-s3");
});
