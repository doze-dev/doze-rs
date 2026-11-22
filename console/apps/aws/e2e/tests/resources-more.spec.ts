// Resources' second part against a real doze-aws: an IAM role made in three steps from the service it is for and deleted, a log
// group's streams, a state machine's executions (newest first), a CloudFormation stack's resources and outputs made with the
// AWS CLI, and Live hiding this console's own calls until asked.
import { calls, CONSOLE, expect, open, openLive, rows, test, unique } from "./fixtures";

test("a role is made in three steps from the service it is for, opened, and deleted", async ({ page, browserName }) => {
  const role = unique("handler", browserName);
  await open(page, "/resources/iam/roles", "Roles");
  await page.getByTestId("create-open").click();
  await expect(page.locator("h1").first()).toHaveText("Select trusted entity");
  // The trusted entity becomes the trust policy, shown as it is built; the name is the last step's.
  await page.getByTestId("type-service").click();
  await expect(page.getByTestId("trust-json")).toContainText("lambda.amazonaws.com");
  await page.getByTestId("form-submit").click();
  await page.getByTestId("form-submit").click();
  await page.getByTestId("role-name").fill(role);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText(`Role ${role} created.`);
  await page.getByTestId("view-created").click();
  await expect(page.locator("h1")).toHaveText(role);
  await page.getByTestId("tab-trust").click();
  await expect(page.getByTestId("trust-json")).toContainText("sts:AssumeRole");
  await page.getByTestId("actions-menu").click();
  await page.getByRole("menuitem", { name: "Delete role" }).click();
  const confirm = page.getByTestId("delete-dialog");
  await expect(confirm).toContainText(role);
  await confirm.getByTestId("delete-dialog-input").fill(role);
  await confirm.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText(`Role ${role} deleted.`);
  await expect(page.getByRole("link", { name: role, exact: true })).toHaveCount(0);
  const made = (await calls(page)).calls.filter((c) => c.caller === "console" && c.action === "CreateRole" && c.resource === role);
  expect(made.map((c) => c.outcome)).toEqual(["ok"]);
});

test("a log group made with the CLI shows its streams", async ({ page, cli, browserName }) => {
  const group = `/e2e/${unique("api", browserName)}`;
  expect(cli(["logs", "create-log-group", "--log-group-name", group]).code).toBe(0);
  for (const s of ["first", "second"]) expect(cli(["logs", "create-log-stream", "--log-group-name", group, "--log-stream-name", s]).code).toBe(0);
  await open(page, `/resources/logs/groups/${encodeURIComponent(group)}`, group);
  await expect(page.getByTestId("field-Retention")).toHaveText("Never Expire");
  // Its streams are the page's first tab, with the count on it.
  await expect(page.getByTestId("tab-streams")).toContainText("2");
  const streams = page.getByTestId("group-streams");
  await expect(streams).toContainText("DescribeLogStreams");
  await expect(streams.locator("tbody tr")).toHaveCount(2);
});

test("a state machine lists its executions, the newest first", async ({ page, cli, browserName }) => {
  const name = unique("checkout", browserName);
  const definition = JSON.stringify({ StartAt: "Done", States: { Done: { Type: "Pass", End: true } } });
  const made = cli(["stepfunctions", "create-state-machine", "--name", name, "--definition", definition, "--role-arn", "arn:aws:iam::111122223333:role/sfn"]);
  expect(made.code).toBe(0);
  const arn = (JSON.parse(made.out) as { stateMachineArn: string }).stateMachineArn;
  for (const n of ["one", "two"]) {
    expect(cli(["stepfunctions", "start-execution", "--state-machine-arn", arn, "--name", n]).code).toBe(0);
    await page.waitForTimeout(1100);
  }
  await open(page, `/resources/stepfunctions/machines/${name}`, name);
  // Its executions are a tab of their own, not a list on its details as well.
  await expect(page.getByTestId("sub-Executions")).toHaveCount(0);
  await page.getByTestId("tab-executions").click();
  const executions = page.getByRole("table", { name: "Executions" }).locator("tbody tr[data-row]");
  await expect(executions).toHaveCount(2);
  await expect(executions.first()).toContainText("two");
  await expect(executions.last()).toContainText("one");
  // The console makes one, in Workflow Studio (the boards' spec, stepfunctions-boards.spec.ts, walks it).
  await open(page, "/resources/stepfunctions/machines", "State machines");
  await expect(page.getByTestId("create-open")).toHaveText("Create state machine");
});

test("a stack made with the CLI shows the resources it made and its outputs", async ({ page, cli, browserName }) => {
  const stack = unique("larder", browserName);
  const queue = unique("stack-inbox", browserName);
  const template = JSON.stringify({ Resources: { Inbox: { Type: "AWS::SQS::Queue", Properties: { QueueName: queue } } }, Outputs: { InboxUrl: { Value: { Ref: "Inbox" }, Description: "Where orders arrive" } } });
  expect(cli(["cloudformation", "create-stack", "--stack-name", stack, "--template-body", template]).code).toBe(0);
  await expect(async () => {
    await open(page, `/resources/cloudformation/stacks/${stack}`, stack);
    await expect(page.getByTestId("detail-badge")).toHaveText("CREATE_COMPLETE", { timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  await page.getByTestId("tab-resources").click();
  await expect(page.getByTestId("stack-resources")).toContainText("AWS::SQS::Queue");
  await page.getByTestId("tab-outputs").click();
  const outputs = page.getByTestId("stack-outputs");
  await expect(outputs).toContainText("InboxUrl");
  await expect(outputs).toContainText(queue);
});

test("Live hides this console's own calls until asked, and the map leaves them out", async ({ page, cli, browserName }) => {
  // The console's reads of a list are its own calls.
  await open(page, "/resources/sqs/queues", "Queues");
  const q = unique("watched", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", q]).code).toBe(0);
  await openLive(page, "/live?view=both");
  const line = page.getByTestId("console-hidden");
  await expect(line).toContainText(/\d+ calls? made by this console (is|are) hidden/);
  await expect(rows(page).filter({ hasText: q }).first()).toBeVisible();
  await expect(rows(page).filter({ hasText: "ListQueues" }).filter({ hasText: "console" })).toHaveCount(0);
  const station = page.locator('[data-testid="map-caller"][data-station="console"]');
  await expect(page.locator('[data-testid="map-caller"]').first()).toBeVisible();
  await expect(station).toHaveCount(0);
  await page.getByTestId("console-toggle").click();
  await expect(page).toHaveURL(/console=shown/);
  await expect(line).toContainText("shown");
  await expect(rows(page).filter({ hasText: "console" }).first()).toBeVisible();
  await expect(station).toHaveCount(1);
  // Kept in the address: a reload shows them still.
  await page.reload();
  await expect(page.getByTestId("console-hidden")).toContainText("shown");
  expect(CONSOLE).not.toBe("");
});
