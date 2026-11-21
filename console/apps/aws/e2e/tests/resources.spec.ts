// Resources against a real doze-aws: the index counts what the services hold, a queue, a table, a bucket and a secret are
// created and deleted through the console (dialogs over the page), each call is in Live as the caller `console`, a secret's
// value is shown only on Reveal, Live's link to a resource lands on its page, and under `--iam enforce` the console still works.
import type { Page } from "@playwright/test";
import { start } from "../server";
import { calls, CONSOLE, ENDPOINT, expect, open, test, unique } from "./fixtures";

const proof = { "X-Console": "1" };

/** The console's calls of `action` in the Live record. */
async function consoleCalls(page: Page, action: string, consoleUrl = CONSOLE) {
  return (await calls(page, consoleUrl)).calls.filter((c) => c.caller === "console" && c.action === action);
}

/** Creates one through the list page's dialog; lands on its page. */
async function create(page: Page, path: string, title: string, fields: Record<string, string>) {
  await open(page, path, title);
  await page.getByTestId("create-open").click();
  const dialog = page.getByTestId("create-dialog");
  await expect(dialog).toBeVisible();
  for (const [key, value] of Object.entries(fields)) await dialog.getByTestId(`field-${key}`).fill(value);
  await dialog.getByTestId("create-submit").click();
  await expect(dialog).toHaveCount(0);
}

/** Deletes the resource whose page is open; lands on its list. */
async function remove(page: Page, name: string) {
  await page.getByTestId("delete-open").click();
  const dialog = page.getByTestId("delete-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(name);
  await expect(dialog.getByTestId("delete-submit")).toBeDisabled();
  await dialog.getByTestId("confirm-delete").fill("delete");
  await dialog.getByTestId("delete-submit").click();
  await expect(dialog).toHaveCount(0);
}

test("the index groups the services and counts what each holds", async ({ page, cli, browserName }) => {
  const q = unique("counted", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", q]).code).toBe(0);
  await open(page, "/resources", "Resources");
  const index = page.getByTestId("resources-index");
  await expect(index.getByRole("heading", { level: 2 })).toContainText(["Messaging and events", "Storage and data", "Compute and flow", "Config and secrets"]);
  await expect(page.getByTestId("count-sqs")).toContainText(/\d+ queues?/);
  await expect(page.getByTestId("count-iam")).toContainText(/\d+ roles?/);
  await expect(page.getByTestId("count-sts")).toHaveText("No resources");
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
  await page.getByTestId("form-submit").click();
}

test("a queue is created on its page, listed, opened, and deleted, each call in Live as the console", async ({ page, browserName }) => {
  const q = unique("orders", browserName);
  await createQueue(page, q);
  await expect(page.locator("h1")).toHaveText(q);
  await expect(page.getByTestId("field-Type")).toHaveText("Standard");
  await expect(page.getByTestId("field-Visibility timeout")).toHaveText("30 seconds");
  // Its recent calls are the console's own, hidden until asked for.
  await expect(page.getByTestId("calls-console-hidden")).toContainText(/\d+ made by this console (is|are) hidden/);
  await page.getByTestId("calls-console-toggle").click();
  await expect(page.getByTestId("recent-calls")).toContainText("CreateQueue");
  await expect(page.getByTestId("recent-calls")).toContainText("console");
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
  await expect(page.getByTestId("deleted-banner")).toContainText(`Queue ${q} deleted.`);
  await expect(page.getByTestId("row-link").filter({ hasText: q })).toHaveCount(0);
  expect((await consoleCalls(page, "DeleteQueue")).length).toBeGreaterThan(0);
});

test.describe("a refusal", () => {
  // The refused create is a 400 the browser logs.
  test.use({ allowErrors: /status of 400|400 \(Bad Request\)/ });

test("a table is created and deleted; a refusal is shown in the dialog", async ({ page, browserName }) => {
  const t = unique("events", browserName);
  await create(page, "/resources/dynamodb/tables", "Tables", { TableName: t, PartitionKey: "pk" });
  await expect(page.locator("h1")).toHaveText(t);
  await expect(page.getByTestId("field-Partition key")).toHaveText("pk");
  // The same name again: DynamoDB's own refusal, in the dialog, which stays open with what was typed.
  await open(page, "/resources/dynamodb/tables", "Tables");
  await page.getByTestId("create-open").click();
  await page.getByTestId("field-TableName").fill(t);
  await page.getByTestId("field-PartitionKey").fill("pk");
  await page.getByTestId("create-submit").click();
  await expect(page.getByTestId("create-error")).toContainText("ResourceInUseException");
  await expect(page.getByTestId("field-TableName")).toHaveValue(t);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("create-dialog")).toHaveCount(0);
  await page.getByTestId("row-link").filter({ hasText: t }).click();
  await remove(page, t);
  await expect(page.getByTestId("row-link").filter({ hasText: t })).toHaveCount(0);

  // (A bucket's own pages, its create and its delete, are s3-boards.spec.ts.)
});
});

test("a secret's value is hidden until Reveal, which is one GetSecretValue in Live", async ({ page, browserName }) => {
  const name = `larder/${unique("stripe", browserName)}`;
  await create(page, "/resources/secretsmanager/secrets", "Secrets", { Name: name, SecretString: "sk_test_e2e" });
  await expect(page.locator("h1")).toHaveText(name);
  await expect(page.getByTestId("detail")).not.toContainText("sk_test_e2e");
  const before = (await consoleCalls(page, "GetSecretValue")).length;
  await page.getByTestId("reveal-value").click();
  await expect(page.getByTestId("revealed-value")).toHaveText("sk_test_e2e");
  expect((await consoleCalls(page, "GetSecretValue")).length).toBe(before + 1);
  await page.getByTestId("hide-value").click();
  await expect(page.getByTestId("revealed-value")).toHaveCount(0);
  await remove(page, name);
  await expect(page.locator("h1")).toHaveText("Secrets");
});

test("a secret marked for deletion is listed, cannot be revealed or deleted again, and its deletion can be cancelled", async ({ page, cli, browserName }) => {
  const name = `larder/${unique("marked", browserName)}`;
  expect(cli(["secretsmanager", "create-secret", "--name", name, "--secret-string", "sk_marked"]).code).toBe(0);
  expect(cli(["secretsmanager", "delete-secret", "--secret-id", name, "--recovery-window-in-days", "7"]).code).toBe(0);
  await open(page, "/resources/secretsmanager/secrets", "Secrets");
  await expect(page.getByTestId("row-link").filter({ hasText: name })).toHaveCount(1);
  await open(page, `/resources/secretsmanager/secrets/${encodeURIComponent(name)}`, name);
  await expect(page.getByTestId("secret-marked")).toContainText("marked for deletion");
  await expect(page.getByTestId("reveal-value")).toHaveCount(0);
  await expect(page.getByTestId("delete-open")).toHaveCount(0);
  await expect(page.getByTestId("detail")).toContainText("Not readable while the secret is marked for deletion");
  await expect(page.getByTestId("detail")).not.toContainText("Scheduled for deletion");
  await page.getByTestId("restore-secret").click();
  await expect(page.getByTestId("secret-marked")).toHaveCount(0);
  await expect(page.getByTestId("reveal-value")).toBeVisible();
  await expect(page.getByTestId("delete-open")).toBeVisible();
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
  await expect(page.getByRole("option", { name: new RegExp(q) })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.locator("h1")).toHaveText(q);
});

test("under --iam enforce the console is allowed: it calls as the account root", async ({ page, browserName }) => {
  const server = await start(Number(process.env.DOZE_AWS_E2E_PORT ?? 42101) + 2, Number(process.env.DOZE_AWS_E2E_CONSOLE_PORT ?? 42102) + 2, { args: ["--iam", "enforce"] });
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
