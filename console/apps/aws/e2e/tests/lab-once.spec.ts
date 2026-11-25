// The Lab's forced events in the page, against a real doze-aws of the test's own and the real AWS CLI: each is a real step of the service,
// and the CLI sees what it did. A refusal in words is shown as one. Every run is in the list of what was forced lately, with a link to the result.
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { ACCOUNT } from "../server";
import { expect, pickFrom, test } from "./fixtures";
import { own } from "./lab-kit";

const open = (page: Page, kind: string) => page.getByTestId(`rehearsal-${kind}`).getByTestId("once-open").click();
const run = async (page: Page) => {
  await page.getByTestId("once-run").click();
  await expect(page.getByTestId("once-result")).toBeVisible();
};
const json = (out: string) => JSON.parse(out) as Record<string, unknown>;

test("resting a service closes it the way idleness does; its next call opens it again with its data, and the wake can be timed", async ({ page }) => {
  await own(page, async (server, cli) => {
    const made = json(cli(["sqs", "create-queue", "--queue-name", "rest-me"]).out) as { QueueUrl: string };
    expect(cli(["sqs", "send-message", "--queue-url", made.QueueUrl, "--message-body", "kept"]).code).toBe(0);
    await page.goto(`${server.console}/lab`);
    await open(page, "rest-service");
    await expect(page.getByTestId("once-dialog")).toBeVisible();
    await expect(page.getByTestId("once-lesson")).toBeVisible();
    await pickFrom(page, "Which service", /^SQS/);
    await expect(page.getByTestId("once-option")).toBeVisible();
    // The wake is timed by default (the option is on).
    await expect(page.getByTestId("once-option")).toBeChecked();
    await run(page);
    await expect(page.getByTestId("once-result")).toContainText("Done");
    await expect(page.getByTestId("once-result")).toContainText(/SQS/);
    await expect(page.getByTestId("once-result-link")).toBeVisible();
    await expect(page.getByTestId("once-result")).toContainText("which included opening the service");
    // Without the timing the next call does not wake it: it rests, and resting it again says it is already resting.
    await page.getByTestId("once-option").click();
    await page.getByTestId("once-run").click();
    await expect(page.getByTestId("once-result")).not.toContainText("which included opening");
    await expect(page.getByTestId("once-result")).toContainText("Done");
    await page.getByTestId("once-run").click();
    await expect(page.getByTestId("once-result")).toContainText("Nothing was done");
    await page.getByTestId("once-close").click();
    // The call after it opens it again, and the queue still holds its message.
    const got = cli(["sqs", "receive-message", "--queue-url", made.QueueUrl]);
    expect(got.out).toContain("kept");
    // What was forced is listed, newest first, with a link to the result.
    await page.reload();
    const rows = page.getByTestId("once-row");
    await expect(rows).toHaveCount(3);
    await expect(rows.first()).toHaveAttribute("data-ok", "false");
    await expect(rows.nth(1)).toHaveAttribute("data-ok", "true");
    await rows.nth(1).getByTestId("once-see").click();
    await expect(page).toHaveURL(/\/live\?.*service=sqs/);
    await page.goBack();
    await page.getByTestId("once-clear").click();
    await expect(page.getByTestId("once-history")).toHaveCount(0);
  });
});

test("a schedule fired now invokes its target by the timer's path and keeps its plan", async ({ page }) => {
  await own(page, async (server, cli) => {
    const q = json(cli(["sqs", "create-queue", "--queue-name", "sched-q"]).out) as { QueueUrl: string };
    const target = JSON.stringify({ Arn: `arn:aws:sqs:us-east-1:${ACCOUNT}:sched-q`, RoleArn: `arn:aws:iam::${ACCOUNT}:role/sched`, Input: '{"from":"schedule"}' });
    const made = cli(["scheduler", "create-schedule", "--name", "nightly", "--schedule-expression", "rate(1 day)", "--flexible-time-window", "Mode=OFF", "--target", target]);
    expect(made.code, made.out).toBe(0);
    await page.goto(`${server.console}/lab`);
    await open(page, "fire-schedule");
    await expect(page.getByTestId("once-preview")).toContainText("default/nightly");
    await run(page);
    await expect(page.getByTestId("once-result")).toContainText("Done");
    const got = cli(["sqs", "receive-message", "--queue-url", q.QueueUrl]);
    expect(got.out).toContain("from");
    await page.getByTestId("once-result-link").click();
    await expect(page).toHaveURL(/\/resources\/scheduler\/schedules\/default%2Fnightly/);
  });
});

test("an in-flight message is brought back with the receive count it had", async ({ page }) => {
  await own(page, async (server, cli) => {
    const q = json(cli(["sqs", "create-queue", "--queue-name", "inflight"]).out) as { QueueUrl: string };
    expect(cli(["sqs", "send-message", "--queue-url", q.QueueUrl, "--message-body", "work"]).code).toBe(0);
    // Nothing is in flight yet: the form says so, and forcing says there is nothing to do.
    await page.goto(`${server.console}/lab`);
    await open(page, "expire-visibility");
    await expect(page.getByTestId("once-preview")).toContainText("inflight");
    await run(page);
    await expect(page.getByTestId("once-result")).toContainText("Nothing was done");
    await page.getByTestId("once-close").click();
    // A consumer takes it for five minutes.
    const first = cli(["sqs", "receive-message", "--queue-url", q.QueueUrl, "--visibility-timeout", "300", "--attribute-names", "ApproximateReceiveCount"]);
    expect(first.out).toContain('"ApproximateReceiveCount": "1"');
    expect(cli(["sqs", "receive-message", "--queue-url", q.QueueUrl]).out.trim()).toBe("");
    await open(page, "expire-visibility");
    await expect(page.getByTestId("once-form").getByText(/1 in flight/)).toBeVisible();
    await run(page);
    await expect(page.getByTestId("once-result")).toContainText("Done");
    await page.getByTestId("once-result-link").click();
    await expect(page).toHaveURL(/\/resources\/sqs\/queues\/inflight/);
    // Receivable again, as the second receive.
    const again = cli(["sqs", "receive-message", "--queue-url", q.QueueUrl, "--attribute-names", "ApproximateReceiveCount"]);
    expect(again.out).toContain('"ApproximateReceiveCount": "2"');
  });
});

test("the time-to-live sweep: a resting DynamoDB is not woken by the form until asked, then the sweep runs and says what it did", async ({ page }) => {
  await own(page, async (server, cli) => {
    await page.goto(`${server.console}/lab`);
    await open(page, "sweep-ttl");
    // Nothing has called DynamoDB: the form says it is resting and does not call it.
    await expect(page.getByTestId("once-resting")).toBeVisible();
    await expect(page.getByTestId("once-run")).toBeDisabled();
    await page.getByTestId("once-look").click();
    await expect(page.getByTestId("once-empty")).toBeVisible();
    await page.getByTestId("once-close").click();
    const create = cli(["dynamodb", "create-table", "--table-name", "sessions", "--attribute-definitions", "AttributeName=id,AttributeType=S", "--key-schema", "AttributeName=id,KeyType=HASH", "--billing-mode", "PAY_PER_REQUEST"]);
    expect(create.code, create.out).toBe(0);
    expect(cli(["dynamodb", "update-time-to-live", "--table-name", "sessions", "--time-to-live-specification", "Enabled=true,AttributeName=expires"]).code).toBe(0);
    const past = String(Math.floor(Date.now() / 1000) - 60);
    expect(cli(["dynamodb", "put-item", "--table-name", "sessions", "--item", JSON.stringify({ id: { S: "old" }, expires: { N: past } })]).code).toBe(0);
    expect(cli(["dynamodb", "put-item", "--table-name", "sessions", "--item", JSON.stringify({ id: { S: "live" }, expires: { N: String(Number(past) + 100_000) } })]).code).toBe(0);
    await open(page, "sweep-ttl");
    await expect(page.getByTestId("once-form").getByText(/sessions/).first()).toBeVisible();
    await run(page);
    // Either it swept (doze deletes when due, so seldom) or it says in words that nothing had expired; the live item stays either way.
    await expect(page.getByTestId("once-result")).toContainText(/Done|Nothing was done/);
    const live = cli(["dynamodb", "get-item", "--table-name", "sessions", "--key", '{"id":{"S":"live"}}']);
    expect(live.out).toContain("live");
    const old = cli(["dynamodb", "get-item", "--table-name", "sessions", "--key", '{"id":{"S":"old"}}']);
    expect(old.out).not.toContain('"old"');
    await page.getByTestId("once-close").click();
    await expect(page.getByTestId("once-row").first()).toHaveAttribute("data-kind", "sweep-ttl");
  });
});

test("a cold start ends a function's warm process, and says so when there is none", async ({ page }) => {
  await own(page, async (server, cli) => {
    const dir = mkdtempSync(join(tmpdir(), "lab-fn-"));
    try {
      writeFileSync(join(dir, "index.py"), 'def handler(event, context):\n    return {"ok": True}\n');
      const zip = join(dir, "fn.zip");
      const { execFileSync } = await import("node:child_process");
      execFileSync("zip", ["-q", "-j", zip, join(dir, "index.py")]);
      cli(["iam", "create-role", "--role-name", "lam", "--assume-role-policy-document", '{"Version":"2012-10-17","Statement":[]}']);
      const made = cli(["lambda", "create-function", "--function-name", "warm", "--runtime", "python3.12", "--handler", "index.handler", "--role", `arn:aws:iam::${ACCOUNT}:role/lam`, "--zip-file", `fileb://${zip}`]);
      expect(made.code, made.out).toBe(0);
      expect(cli(["lambda", "invoke", "--function-name", "warm", join(dir, "out.json")]).code).toBe(0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
    await page.goto(`${server.console}/lab`);
    await open(page, "cold-start");
    await expect(page.getByTestId("once-preview")).toContainText("warm");
    await run(page);
    await expect(page.getByTestId("once-result")).toContainText("Done");
    // Again: there is no warm process left, so the next call is already cold.
    await page.getByTestId("once-run").click();
    await expect(page.getByTestId("once-result")).toContainText("Nothing was done");
    await page.getByTestId("once-close").click();
    await expect(page.getByTestId("once-row")).toHaveCount(2);
  });
});

test("a forced event the run cannot carry out is off, with the reason", async ({ page }) => {
  await own(
    page,
    async (server) => {
      await page.goto(`${server.console}/lab`);
      await expect(page.getByTestId("rehearsal-fire-schedule").getByTestId("once-open")).toBeDisabled();
      await expect(page.getByTestId("rehearsal-fire-schedule").getByTestId("once-why-not")).toContainText("not served");
      await expect(page.getByTestId("rehearsal-expire-visibility").getByTestId("once-open")).toBeEnabled();
    },
    ["--services", "sqs,dynamodb"],
  );
});
