// Live's map, the cause of a call between services, the wire's filters and repeats, and copying a call, against the real
// doze-aws and the real AWS CLI.
import { LISTEN_PORT } from "../global-setup";
import { ACCOUNT, aws, type Running, start } from "../server";
import { calls, ENDPOINT, expect, openLive, rows, test, unique } from "./fixtures";

/** A queue subscribed to a topic, and one message published to the topic: SNS delivers it to SQS on doze-aws's bus. */
function fanOut(cli: (args: string[]) => { code: number; out: string }, browserName: string) {
  const queue = unique("picks", browserName);
  const topic = unique("placed", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", queue]).code).toBe(0);
  expect(cli(["sns", "create-topic", "--name", topic]).code).toBe(0);
  const topicArn = `arn:aws:sns:us-east-1:${ACCOUNT}:${topic}`;
  expect(cli(["sns", "subscribe", "--topic-arn", topicArn, "--protocol", "sqs", "--notification-endpoint", `arn:aws:sqs:us-east-1:${ACCOUNT}:${queue}`]).code).toBe(0);
  expect(cli(["sns", "publish", "--topic-arn", topicArn, "--message", "hello"]).code).toBe(0);
  return { queue, topic, topicArn };
}

test("SNS delivering to SQS is a call via SNS, and its drawer shows the Publish that caused it", async ({ page, cli, accessKey, browserName }) => {
  const { queue, topicArn } = fanOut(cli, browserName);
  await openLive(page, "/live?view=wire");
  const record = (await calls(page)).calls;
  const publish = record.filter((c) => c.action === "Publish" && c.resource === topicArn).pop()!;
  const sent = record.find((c) => c.via?.request_id === publish.request_id)!;
  expect(sent.action).toBe("SendMessage");
  expect(sent.via?.by).toBe("sns");
  expect(sent.caller).toBe(accessKey);

  const row = page.locator(`tr[data-row="${sent.seq}"]`);
  await expect(row.locator("td").nth(2)).toHaveText("SendMessage");
  await expect(row.locator("td").nth(3)).toHaveText(queue);
  await expect(row.getByTestId("via-pill")).toHaveText("via");
  await expect(row.locator("td").nth(4)).toContainText("SNS");

  await row.getByTestId("open-call").click();
  const chain = page.getByTestId("cause-chain");
  await expect(chain).toBeVisible();
  await expect(page.getByTestId("call-via")).toContainText("SNS made it on doze-aws's internal bus");
  await expect(chain.getByTestId("cause-step").first()).toContainText("Publish");
  await expect(chain).toContainText("SNS called SQS");
  await expect(chain.getByTestId("cause-here")).toContainText("SendMessage");
  await expect(page.getByTestId("call-request-id")).toContainText("None: a call between services has no request of its own.");
  // The step opens the Publish's own drawer, which lists what it made.
  await chain.getByTestId("cause-step").first().click();
  await expect(page).toHaveURL(new RegExp(`call=${publish.seq}`));
  await expect(page.getByTestId("call-title")).toHaveText("Publish");
  await expect(page.getByTestId("cause-chain").getByTestId("cause-step")).toContainText("SendMessage");
  // What it acted on opens in Resources.
  await expect(page.getByTestId("call-open-resource")).toHaveAttribute("href", /\/resources\/sns\/placed-/);
});

// The map draws the busiest 8 callers: on the shared doze-aws the other tests' callers (each browser's key, IAM's users) push
// this one's off it. So the map is read on a doze-aws of its own (the ports after deep-run's), whose only callers are its own.
test.describe("the map, on a doze-aws of its own", () => {
  let own: Running | undefined;
  test.beforeAll(async () => {
    own = await start(LISTEN_PORT + 4, LISTEN_PORT + 5);
  });
  test.afterAll(async () => {
    await own?.stop();
  });

  test("the map draws a line from each caller to each service it called, and a dashed one for SNS to SQS", async ({ page, accessKey, browserName }) => {
    const cli = (args: string[]) => aws(own?.endpoint ?? "", args, { accessKey });
    fanOut(cli, browserName);
    await openLive(page, `${own?.console}/live?view=map&by=service`);
    await expect(page.getByTestId("live-map")).toBeVisible();
    // The wire is not drawn in the Map view.
    await expect(rows(page)).toHaveCount(0);
    const line = (key: string) => page.locator(`[data-testid="map-line"][data-flow="${key}"]`);
    await expect(line(`key|${accessKey}|sqs`)).toHaveCount(1);
    await expect(line(`key|${accessKey}|sns`)).toHaveCount(1);
    await expect(line("via|sns|sqs")).toHaveAttribute("stroke-dasharray", /\d/);
    await expect(line(`key|${accessKey}|sqs`)).not.toHaveAttribute("stroke-dasharray", /./);
    await expect(page.locator('[data-testid="map-caller"][data-station="sns"]')).toContainText("service");
    // A new call arrives on the open map: its line is there (a new flow is drawn without a reload).
    expect(cli(["dynamodb", "list-tables"]).code).toBe(0);
    await expect(line(`key|${accessKey}|dynamodb`)).toHaveCount(1);
    // A station is a filter: the address keeps it, and Both shows the wire filtered by it.
    await page.locator('[data-testid="map-service"][data-station="dynamodb"]').click();
    await expect(page).toHaveURL(/service=dynamodb/);
    await page.getByTestId("view-both").click();
    await expect(page).toHaveURL(/service=dynamodb/);
    await expect(page).not.toHaveURL(/view=/);
    await expect(rows(page).first().locator("td").nth(1)).toHaveText("DynamoDB");
    for (const cell of await rows(page).locator("td:nth-child(2)").allTextContents()) expect(cell).toBe("DynamoDB");
  });
});

test("the wire's filters keep the calls that match, and live in the address", async ({ page, cli, accessKey, browserName }) => {
  const queue = unique("filters", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", queue]).code).toBe(0);
  expect(cli(["sqs", "get-queue-url", "--queue-name", `${queue}-gone`]).code).not.toBe(0);
  await openLive(page, "/live?view=wire");
  await page.getByTestId("filter-q").fill(queue);
  await expect(page).toHaveURL(new RegExp(`q=${queue}`));
  await expect(rows(page)).toHaveCount(2);
  await expect(page.getByTestId("filter-count")).toContainText("2 of");
  await page.getByTestId("filter-outcome").click();
  await page.getByRole("option", { name: "Error" }).click();
  await expect(page).toHaveURL(/outcome=error/);
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first().locator("td").nth(2)).toHaveText("GetQueueUrl");
  // The address alone shows the same (a reload, a link).
  await page.reload();
  await expect(page.getByTestId("filter-q")).toHaveValue(queue);
  await expect(rows(page)).toHaveCount(1);
  await page.getByTestId("filter-clear").click();
  await expect(page).not.toHaveURL(/q=|outcome=/);
  await expect(page.getByTestId("filter-q")).toHaveValue("");
  // The caller filter knows this browser's key.
  await page.getByTestId("filter-caller").click();
  await page.getByRole("option", { name: accessKey }).click();
  await expect(page).toHaveURL(new RegExp(`caller=${accessKey}`));
  for (const cell of await rows(page).locator("td:nth-child(5)").allTextContents()) expect([accessKey, "SNS"].some((k) => cell.includes(k))).toBe(true);
  // Nothing matches: said, not an empty box.
  await page.getByTestId("filter-q").fill("no-such-operation-anywhere");
  await expect(page.getByText("No call matches")).toBeVisible();
});

test("calls in a row that repeat are one row with a count, opened to show each", async ({ page, cli }) => {
  await openLive(page, "/live?view=wire");
  for (let i = 0; i < 3; i++) expect(cli(["kms", "list-keys"]).code).toBe(0);
  const first = rows(page).first();
  await expect(first.locator("td").nth(2)).toContainText("ListKeys");
  await expect(first.getByTestId("repeat-count")).toHaveText("×3");
  const before = await rows(page).count();
  await first.getByTestId("repeat-count").click();
  await expect(first.getByTestId("repeat-count")).toHaveAttribute("aria-expanded", "true");
  await expect(rows(page)).toHaveCount(before + 2);
  await first.getByTestId("repeat-count").click();
  await expect(rows(page)).toHaveCount(before);
});

test("a call is copied as an AWS CLI command and as curl", async ({ page, cli, browserName, accessKey }) => {
  test.skip(browserName !== "chromium", "the clipboard is read back in Chromium only (the other two give a page no read permission)");
  const queue = unique("copy", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", queue]).code).toBe(0);
  await openLive(page, "/live?view=wire");
  const row = rows(page).filter({ hasText: queue }).first();
  await expect(row.locator("td").nth(4)).toHaveText(accessKey);
  await row.getByTestId("row-menu").click();
  await page.getByTestId("copy-cli").click();
  await expect(page.getByText("AWS CLI command copied")).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(`aws --endpoint-url ${ENDPOINT} --region us-east-1 sqs create-queue --queue-name ${queue}`);
  await row.getByTestId("open-call").click();
  await page.getByTestId("call-copy-curl").click();
  await expect(page.getByText("Curl command copied")).toBeVisible();
  const curl = await page.evaluate(() => navigator.clipboard.readText());
  expect(curl).toContain("X-Amz-Target: AmazonSQS.CreateQueue");
  expect(curl).toContain(`{"QueueName":"${queue}"}`);
  expect(curl).toContain("--aws-sigv4 aws:amz:us-east-1:sqs");
});
