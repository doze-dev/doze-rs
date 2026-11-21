// Home, Access, Lab and Settings against real doze-aws processes, called with the real AWS CLI: the connected check turning
// over when a call arrives, a call soft IAM would refuse grouped with IAM's reason and its drafted policy put on the user, an
// enforced refusal, a call sent from the Lab found in Live, and the settings as /about says them.
import { LISTEN_PORT } from "../global-setup";
import { aws, start } from "../server";
import { about, calls, ENDPOINT, expect, open, test, unique } from "./fixtures";

const offset = (browserName: string) => ({ firefox: 0, webkit: 2, chromium: 4 }[browserName] ?? 6);

/** An IAM user with no policy at `endpoint`, and its access key. */
function nobody(endpoint: string, name: string): string {
  expect(aws(endpoint, ["iam", "create-user", "--user-name", name]).code).toBe(0);
  const made = aws(endpoint, ["iam", "create-access-key", "--user-name", name]);
  expect(made.code, made.out).toBe(0);
  return (JSON.parse(made.out) as { AccessKey: { AccessKeyId: string } }).AccessKey.AccessKeyId;
}

test("Home turns to connected when an app's first call arrives, and names its client", async ({ page, browserName }) => {
  const at = offset(browserName);
  const server = await start(LISTEN_PORT + 40 + at, LISTEN_PORT + 41 + at);
  try {
    await page.goto(`${server.console}/`);
    await expect(page.locator("h1").first()).toHaveText("Home");
    await expect(page.getByTestId("connected")).toHaveAttribute("data-state", "no");
    await expect(page.getByTestId("recent-empty")).toBeVisible();
    // The console's own reads do not count; an app's call does, and the page turns over without a reload.
    expect(aws(server.endpoint, ["sqs", "list-queues"], { accessKey: "AKIAHOMEAPP000000001" }).code).toBe(0);
    await expect(page.getByTestId("connected")).toHaveAttribute("data-state", "yes", { timeout: 15_000 });
    const caller = page.getByTestId("connected-caller").first();
    await expect(caller).toHaveAttribute("data-key", "AKIAHOMEAPP000000001");
    await expect(caller.getByTestId("caller-sdks")).toContainText("AWS CLI");
    await expect(page.getByRole("table", { name: "Recent calls" }).locator("tbody tr[data-row]")).toHaveCount(1);
    // Errors in a burst are worth a look, and say where.
    for (let i = 0; i < 3; i++) aws(server.endpoint, ["sqs", "get-queue-url", "--queue-name", "missing"], { accessKey: "AKIAHOMEAPP000000001" });
    await page.getByTestId("refresh-now").click();
    await expect(page.locator('[data-testid="signal"][data-kind="errors"]')).toContainText("3 SQS errors");
  } finally {
    await page.goto("about:blank"); // the page stops polling before its server goes (WebKit reports the refused poll)
    await server.stop();
  }
});

test("soft IAM: a refused caller is grouped with IAM's reason, Live names it, and the drafted policy lets it through", async ({ page, browserName }) => {
  const user = unique("reporter", browserName);
  const key = nobody(ENDPOINT, user);
  expect(aws(ENDPOINT, ["sqs", "list-queues"], { accessKey: key }).code).toBe(0);
  await open(page, "/access", "Access");
  await expect(page.getByTestId("iam-mode")).toHaveAttribute("data-mode", "soft");
  const group = page.locator(`[data-testid="denial-group"][data-caller="${key}"][data-action="ListQueues"]`);
  await expect(group.getByTestId("denial-caller")).toHaveText(user);
  await expect(group).toContainText("1 call would be denied");
  await expect(group.getByTestId("denial-why")).toContainText("sqs:ListQueues");
  await expect(group.getByTestId("denial-why")).toContainText("no identity-based policy");

  await group.getByTestId("make-pass").click();
  const dialog = page.getByTestId("pass-dialog");
  await expect(dialog.getByTestId("drafted-policy")).toContainText('"sqs:ListQueues"');
  await expect(dialog).toContainText(`Put it on user ${user}`);
  await dialog.getByTestId("attach-submit").click();
  await expect(dialog).toHaveCount(0);
  // The same call again: IAM now allows it.
  expect(aws(ENDPOINT, ["sqs", "list-queues"], { accessKey: key }).code).toBe(0);
  const mine = (await calls(page)).calls.filter((c) => c.caller === key && c.action === "ListQueues");
  expect(mine.map((c) => c.would_be_denied)).toEqual([true, false]);
  expect(mine[1]!.sdk).toMatch(/^AWS CLI /);

  // Live shows the user's name in place of its key, and the key on hover.
  await page.goto(`/live?view=wire&caller=${key}`);
  const name = page.getByTestId("caller-name").first();
  await expect(name).toHaveText(user);
  await expect(name).toHaveAttribute("title", new RegExp(key));
});

test("enforced IAM: a refused call is listed as denied", async ({ page, browserName }) => {
  const at = offset(browserName);
  const server = await start(LISTEN_PORT + 50 + at, LISTEN_PORT + 51 + at, { args: ["--iam", "enforce"] });
  try {
    const key = nobody(server.endpoint, "nobody");
    expect(aws(server.endpoint, ["sqs", "list-queues"], { accessKey: key }).code).not.toBe(0);
    await page.goto(`${server.console}/access`);
    await expect(page.locator("h1").first()).toHaveText("Access");
    await expect(page.getByTestId("denials").getByRole("heading", { name: "Denied" })).toBeVisible();
    const group = page.locator(`[data-testid="denial-group"][data-caller="${key}"]`);
    await expect(group).toContainText("1 call denied");
    await expect(group.getByTestId("denial-caller")).toHaveText("nobody");
  } finally {
    await page.goto("about:blank"); // the page stops polling before its server goes (WebKit reports the refused poll)
    await server.stop();
  }
});

test("the Lab sends a built operation from the model's input and opens its call in Live", async ({ page, browserName }) => {
  // Not "lab": the palette test types that and expects the space alone.
  const queue = unique("tried", browserName);
  await open(page, "/lab?api=sqs&op=CreateQueue", "Lab");
  const input = page.getByTestId("lab-input");
  await expect(input).toHaveValue(/"QueueName": ""/);
  await input.fill(JSON.stringify({ QueueName: queue }));
  await page.getByTestId("lab-send").click();
  await expect(page.getByTestId("lab-output")).toContainText(queue);
  await page.getByTestId("lab-in-live").click();
  await expect(page.getByTestId("call-sheet")).toBeVisible();
  await expect(page.getByTestId("call-sheet")).toContainText("CreateQueue");
  // A JSON that is not an object is not sent.
  await page.goto("/lab?api=sqs&op=CreateQueue");
  await page.getByTestId("lab-input").fill("[1]");
  await expect(page.getByTestId("lab-send")).toBeDisabled();
});

test("Settings says what /about says, every setting with where it came from, and the data directory", async ({ page }) => {
  await open(page, "/settings", "Settings");
  const a = await about(page);
  await expect(page.getByTestId("fact-version")).toContainText(a.version);
  await expect(page.getByTestId("fact-account")).toContainText(a.account);
  await expect(page.getByTestId("fact-region")).toContainText(a.region);
  await expect(page.getByTestId("fact-auth")).toContainText(a.auth);
  await expect(page.getByTestId("fact-iam")).toContainText(a.iam);
  await expect(page.getByTestId("fact-console")).toContainText(a.console);
  await expect(page.getByTestId("fact-process")).toContainText(`pid ${a.pid}`);
  await expect(page.getByTestId("fact-data-dir")).toContainText(a.data_dir);
  expect(a.data_dir).toBe(process.env.DOZE_AWS_E2E_DATA_DIR);
  const flags = page.getByRole("table", { name: "Settings" });
  await expect(flags.locator('tr[data-row="account"]')).toContainText("--account");
  await expect(flags.locator('tr[data-row="iam"]')).toContainText("the default iam");
  await expect(flags.locator('tr[data-row="secret-key"]')).toContainText("hidden (a secret)");
  await page.getByTestId("flags-changed-only").click();
  await expect(flags.locator('tr[data-row="iam"]')).toHaveCount(0);
  // Compact tables are kept in this browser.
  await page.getByTestId("density-switch").click();
  await expect(page.locator("html")).toHaveAttribute("data-density", "compact");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-density", "compact");
  await page.getByTestId("density-switch").click();
  await expect(page.locator("html")).not.toHaveAttribute("data-density", "compact");
});
