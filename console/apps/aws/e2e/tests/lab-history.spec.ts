// The Lab's memory of this browser: the calls it sent (replayed, sent again, cleared) and the inputs saved under a name. Both are kept in the
// browser for the region chosen; doze-aws does not know them. The calls are real (CreateQueue), and the AWS CLI sees their effect.
import { ACCOUNT } from "../server";
import { ENDPOINT, expect, open, overflowX, test, unique } from "./fixtures";

const rows = (page: import("@playwright/test").Page) => page.getByTestId("lab-history-row");

test("a call sent is in the history, kept across a reload; Replay loads its input, Send again sends it as it was, Clear empties it", async ({ page, cli, browserName }) => {
  const name = unique("hist", browserName);
  const url = `${ENDPOINT}/${ACCOUNT}/${name}`;
  expect(cli(["sqs", "create-queue", "--queue-name", name]).code).toBe(0);
  const waiting = () => JSON.parse(cli(["sqs", "get-queue-attributes", "--queue-url", url, "--attribute-names", "ApproximateNumberOfMessages"]).out).Attributes.ApproximateNumberOfMessages as string;
  await open(page, "/lab?tab=send&api=sqs&op=SendMessage", "Lab");
  await expect(page.getByTestId("lab-history-empty")).toBeVisible();
  await page.getByTestId("lab-input").fill(JSON.stringify({ QueueUrl: url, MessageBody: "from the history test" }));
  await page.getByTestId("lab-send").click();
  await expect(page.getByTestId("lab-answer")).toContainText("Answered");
  await expect(rows(page)).toHaveCount(1);
  const first = rows(page).first();
  await expect(first).toContainText("SQS SendMessage");
  await expect(first).toHaveAttribute("data-outcome", "ok");
  await expect(first.getByTestId("lab-history-summary")).toContainText(`QueueUrl: "${name}"`);
  await expect.poll(waiting).toBe("1");
  // Kept in this browser: a reload, and another operation, still show it.
  await open(page, "/lab?tab=send&api=sqs&op=ListQueues", "Lab");
  await expect(rows(page)).toHaveCount(1);
  // Replay: the editor opens on that call's operation, with its input.
  await rows(page).first().getByTestId("lab-replay").click();
  await expect(page).toHaveURL(/op=SendMessage/);
  await expect(page.getByTestId("lab-input")).toHaveValue(new RegExp(name));
  await expect(page.getByTestId("lab-note")).toContainText("Replayed");
  // Send again: a real call, a second row, a second message.
  await rows(page).first().getByTestId("lab-send-again").click();
  await expect(rows(page)).toHaveCount(2);
  await expect(page.getByTestId("lab-answer")).toContainText("Answered");
  await expect.poll(waiting).toBe("2");
  // Clear.
  await page.getByTestId("lab-history-clear").click();
  await expect(page.getByTestId("lab-history-empty")).toBeVisible();
  await page.reload();
  await expect(page.getByTestId("lab-history-empty")).toBeVisible();
});

test("a refused call is in the history with the service's code, and each region keeps a history of its own", async ({ page, browserName }) => {
  const missing = unique("nothere", browserName);
  await open(page, "/lab?tab=send&api=sqs&op=GetQueueUrl", "Lab");
  await page.getByTestId("lab-input").fill(JSON.stringify({ QueueName: missing }));
  await page.getByTestId("lab-send").click();
  await expect(page.getByTestId("lab-refusal")).toBeVisible();
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toHaveAttribute("data-outcome", "refused");
  await expect(rows(page).first()).not.toContainText("Answered");
  // Another region: its history is empty, and the first region's is still there when it is chosen again.
  await page.evaluate(() => localStorage.setItem("doze-aws.region", "eu-west-1"));
  await open(page, "/lab?tab=send&api=sqs&op=ListQueues", "Lab");
  await expect(page.getByTestId("lab-history-empty")).toBeVisible();
  await page.evaluate(() => localStorage.removeItem("doze-aws.region"));
  await open(page, "/lab?tab=send&api=sqs&op=ListQueues", "Lab");
  await expect(rows(page)).toHaveCount(1);
});

test("an input is saved under a name, kept across a reload, opened into the editor and deleted; a bad name is refused in words", async ({ page, browserName }) => {
  const name = unique("saved", browserName);
  await open(page, "/lab?tab=send&api=sqs&op=CreateQueue", "Lab");
  await expect(page.getByTestId("lab-saved-empty")).toBeVisible();
  await page.getByTestId("lab-input").fill('{"QueueName": "saved-queue-input"}');
  await page.getByTestId("lab-save-open").click();
  await page.getByTestId("lab-save-confirm").click();
  await expect(page.getByTestId("lab-save-why")).toHaveText("Give it a name.");
  await page.getByTestId("lab-save-name").fill(name);
  await page.getByTestId("lab-save-name").press("Enter");
  const saved = page.locator(`[data-testid="lab-saved-row"][data-name="${name}"]`);
  await expect(saved).toBeVisible();
  await expect(saved).toContainText("SQS CreateQueue");
  // Saving under the same name again replaces it, it does not add one.
  await page.getByTestId("lab-input").fill('{"QueueName": "saved-queue-again"}');
  await page.getByTestId("lab-save-open").click();
  await page.getByTestId("lab-save-name").fill(name.toUpperCase());
  await page.getByTestId("lab-save-confirm").click();
  await expect(page.getByTestId("lab-saved-row")).toHaveCount(1);
  await page.reload();
  await expect(page.getByTestId("lab-saved-row")).toHaveCount(1);
  // Open: another operation's editor becomes the saved one's.
  await open(page, "/lab?tab=send&api=sqs&op=ListQueues", "Lab");
  await page.getByTestId("lab-saved-open").click();
  await expect(page).toHaveURL(/op=CreateQueue/);
  await expect(page.getByTestId("lab-input")).toHaveValue(/saved-queue-again/);
  await page.getByTestId("lab-saved-delete").click();
  await expect(page.getByTestId("lab-saved-empty")).toBeVisible();
  await page.reload();
  await expect(page.getByTestId("lab-saved-empty")).toBeVisible();
});

test("a kept list that was edited by hand shows what is whole of it, and the page does not scroll sideways with a long history", async ({ page }) => {
  await open(page, "/lab?tab=send&api=sqs&op=ListQueues", "Lab");
  const long = "x".repeat(400);
  await page.evaluate((text) => {
    const row = (i: number) => ({ id: `r${i}`, at: Date.now() - i * 1000, target: "sqs", service: "sqs", title: "SQS", op: "SendMessage", input: { QueueUrl: "http://127.0.0.1:1/1/q", MessageBody: text }, ok: i % 3 !== 0, code: i % 3 === 0 ? "AWS.SimpleQueueService.NonExistentQueue" : null, status: null, took_us: 1500, summary: `QueueUrl: "q", MessageBody: "${text.slice(0, 20)}…"` });
    localStorage.setItem("doze-aws.lab.sent.default", JSON.stringify([...Array.from({ length: 6 }, (_, i) => row(i)), { id: "broken" }, 7, null]));
  }, long);
  await page.reload();
  await expect(rows(page)).toHaveCount(6);
  for (const width of [1440, 1024]) {
    await page.setViewportSize({ width, height: 900 });
    await expect(rows(page).first()).toBeVisible();
    expect(await overflowX(page), `at ${width}`).toBe(0);
  }
});
