// Resources against a real doze-aws: the index counts what the services hold, a queue, a table, a bucket and a secret are
// created and deleted through the console (dialogs over the page), each call is in Live as the caller `console`, a secret's
// value is shown only on Reveal, Live's link to a resource lands on its page, and under `--iam enforce` the console still works.
import type { Page } from "@playwright/test";
import { start } from "../server";
import { advance, calls, CONSOLE, ENDPOINT, expect, expectNoErrorAnswers, open, test, unique } from "./fixtures";

const proof = { "X-Console": "1" };

/** The console's calls of `action` in the Live record. */
async function consoleCalls(page: Page, action: string, consoleUrl = CONSOLE) {
  return (await calls(page, consoleUrl)).calls.filter((c) => c.caller === "console" && c.action === action);
}

test("the index groups the services and counts what each holds", async ({ page, cli, browserName }) => {
  const q = unique("counted", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", q]).code).toBe(0);
  await open(page, "/resources", "Resources");
  const index = page.getByTestId("resources-index");
  await expect(index.getByRole("heading", { level: 2 })).toContainText(["Messaging and events", "Storage and data", "Compute and flow", "Config and secrets"]);
  await expect(page.getByTestId("count-sqs")).toContainText(/\d+ queues?/);
  // IAM and STS are the Access space's: the board draws them there only.
  await expect(page.getByTestId("service-iam")).toHaveCount(0);
  await expect(page.getByTestId("service-sts")).toHaveCount(0);
  // Each count was a list call the console made.
  expect((await consoleCalls(page, "ListQueues")).length).toBeGreaterThan(0);
  await page.getByTestId("service-sqs").click();
  await expect(page.locator("h1")).toHaveText("Queues");
  await expect(page.getByTestId("row-link").filter({ hasText: q })).toBeVisible();
});

/** Creates a queue on its own page (Create queue); lands on its page. */
async function createQueue(page: Page, name: string, consoleUrl = "") {
  await page.goto(`${consoleUrl}/resources/sqs/queues`);
  await expect(page.locator("h1")).toHaveText("Queues");
  await page.getByTestId("create-open").click();
  await page.getByTestId("queue-name").fill(name);
  await advance(page);
}

test("a queue is created on its page, listed, opened, and deleted, each call in Live as the console", async ({ page, browserName, refusals }) => {
  const q = unique("orders", browserName);
  await createQueue(page, q);
  await expect(page.locator("h1")).toHaveText(q);
  await expect(page.getByTestId("field-Type")).toHaveText("Standard");
  await expect(page.getByTestId("field-Visibility timeout")).toHaveText("30 seconds");
  // Its recent calls are the console's own write, in view (the console's reads are hidden until asked for: fix-shell.spec.ts).
  await expect(page.getByTestId("recent-calls")).toContainText("CreateQueue");
  await expect(page.getByTestId("recent-calls")).toContainText("This console");
  // The columns are the boards': Time, Caller, Call, Result.
  await expect(page.getByRole("table", { name: "Recent calls" }).locator("th")).toHaveText(["Time", "Caller", "Call", "Result"]);
  const made = (await consoleCalls(page, "CreateQueue")).find((c) => c.resource === q);
  expect(made?.outcome).toBe("ok");
  // Listed, and found by the search.
  await open(page, "/resources/sqs/queues", "Queues");
  await page.getByTestId("list-search").fill(q);
  await page.getByTestId("list-search-go").click();
  await expect(page.getByTestId("row-link")).toHaveText([q]);
  await page.getByTestId("row-link").click();
  await expect(page.locator("h1")).toHaveText(q);
  await page.getByTestId("verb-delete").click();
  await page.getByTestId("delete-dialog-input").fill("delete");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.locator("h1")).toHaveText("Queues");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("deleted-banner")).toContainText(`Queue ${q} deleted.`);
  await expect(page.getByTestId("row-link").filter({ hasText: q })).toHaveCount(0);
  expect((await consoleCalls(page, "DeleteQueue")).length).toBeGreaterThan(0);
});

test("a table is created and deleted; a refusal is shown on the page", async ({ page, browserName, refusals }) => {
  // The name that is taken is DynamoDB's refusal, shown on the form.
  refusals.allow(/400 POST .*\/dynamodb\/tables\/op\/CreateTable$/);
  const t = unique("events", browserName);
  await open(page, "/resources/dynamodb/tables?do=create", "Create table");
  await page.getByTestId("table-name").fill(t);
  await page.getByTestId("table-pk").fill("pk");
  await advance(page);
  await expect(page.getByTestId("created-banner")).toContainText(`${t} was created and is Active.`);
  // The same name again: DynamoDB's own refusal, on the page, which keeps what was typed.
  await page.getByTestId("create-open").click();
  await page.getByTestId("table-name").fill(t);
  await page.getByTestId("table-pk").fill("pk");
  await advance(page);
  await expect(page.getByTestId("create-refused")).toContainText("ResourceInUseException");
  await expect(page.getByTestId("table-name")).toHaveValue(t);
  await page.getByTestId("form-cancel").click();
  await expect(page.locator("h1")).toHaveText("Tables");
  await page.getByTestId(`select-${t}`).click();
  await page.getByTestId("table-delete").click();
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText(`${t}`);
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByTestId("row-link").filter({ hasText: t })).toHaveCount(0);

  // (A bucket's own pages, its create and its delete, are s3-boards.spec.ts.)
});

test("a secret's value is hidden until Retrieve secret value, which is one GetSecretValue in Live", async ({ page, cli, browserName }) => {
  const name = `larder/${unique("stripe", browserName)}`;
  expect(cli(["secretsmanager", "create-secret", "--name", name, "--secret-string", "sk_test_e2e"]).code).toBe(0);
  await open(page, `/resources/secretsmanager/secrets/${encodeURIComponent(name)}`, name);
  await expect(page.getByTestId("secret-value")).not.toContainText("sk_test_e2e");
  const before = (await consoleCalls(page, "GetSecretValue")).length;
  await page.getByTestId("value-retrieve").click();
  await expect(page.getByTestId("secret-value")).toContainText("sk_test_e2e");
  expect((await consoleCalls(page, "GetSecretValue")).length).toBe(before + 1);
  await page.getByTestId("value-hide").click();
  await expect(page.getByTestId("secret-value")).not.toContainText("sk_test_e2e");
  expect(cli(["secretsmanager", "delete-secret", "--secret-id", name, "--force-delete-without-recovery"]).code).toBe(0);
});

test("a secret marked for deletion is listed only on asking, cannot be retrieved, and its deletion can be cancelled", async ({ page, cli, browserName }) => {
  const name = `larder/${unique("marked", browserName)}`;
  expect(cli(["secretsmanager", "create-secret", "--name", name, "--secret-string", "sk_marked"]).code).toBe(0);
  // A live secret too: a list with nothing but a scheduled one says "No secrets", as the board does.
  expect(cli(["secretsmanager", "create-secret", "--name", `larder/${unique("live", browserName)}`, "--secret-string", "x"]).code).toBe(0);
  expect(cli(["secretsmanager", "delete-secret", "--secret-id", name, "--recovery-window-in-days", "7"]).code).toBe(0);
  await open(page, "/resources/secretsmanager/secrets", "Secrets");
  await expect(page.getByTestId("row-link").filter({ hasText: name })).toHaveCount(0);
  await page.getByTestId("preferences").click();
  await page.getByTestId("show-deleted").click();
  await expect(page.getByTestId("row-link").filter({ hasText: name })).toHaveCount(1);
  await open(page, `/resources/secretsmanager/secrets/${encodeURIComponent(name)}`, name);
  await expect(page.getByTestId("secret-scheduled")).toContainText("Scheduled for deletion");
  await expect(page.getByTestId("value-retrieve")).toHaveCount(0);
  await expect(page.getByTestId("value-scheduled")).toContainText("You can't retrieve the value");
  await page.getByTestId("cancel-deletion").click();
  await expect(page.getByTestId("secret-scheduled")).toHaveCount(0);
  await expect(page.getByTestId("value-retrieve")).toBeVisible();
});

test("Live's link to what a call acted on opens its page in Resources", async ({ page, cli, browserName }) => {
  const q = unique("linked", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", q]).code).toBe(0);
  const record = (await calls(page)).calls.filter((c) => c.action === "CreateQueue" && c.resource === q).pop()!;
  await page.goto(`/live?call=${record.seq}`);
  await page.getByTestId("call-open-resource").click();
  await expect(page.locator("h1")).toHaveText(q);
  await expect(page).toHaveURL(new RegExp(`/resources/sqs/${q}\\?m=QueueName$`));
  await expect(page.getByTestId("field-Type")).toHaveText("Standard");
});

test("the palette jumps to a resource by name", async ({ page, cli, browserName }) => {
  const q = unique("palette", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", q]).code).toBe(0);
  await open(page, "/", "Home");
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.type(q);
  await expect(page.getByRole("option", { name: q, exact: true })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.locator("h1")).toHaveText(q);
});

test("under --iam enforce the console is allowed: it calls as the account root", async ({ page, browserName }) => {
  const server = await start(Number(process.env.DOZE_AWS_E2E_PORT ?? 20002) + 2, Number(process.env.DOZE_AWS_E2E_CONSOLE_PORT ?? 20003) + 2, { args: ["--iam", "enforce"] });
  try {
    const q = unique("enforced", browserName);
    await createQueue(page, q, server.console);
    await expect(page.locator("h1")).toHaveText(q);
    const made = (await consoleCalls(page, "CreateQueue", server.console)).pop();
    expect(made?.outcome).toBe("ok");
    const index = await (await page.request.get(`${server.console}/api/v1/resources/sqs/queues`, { headers: proof })).json();
    expect(JSON.stringify(index)).toContain(q);
  } finally {
    await page.goto("about:blank"); // the page stops polling before its server goes (WebKit reports the refused poll)
    await server.stop();
  }
  expect(ENDPOINT).not.toBe("");
});
