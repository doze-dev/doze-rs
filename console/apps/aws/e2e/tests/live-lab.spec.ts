// A Lab rule on the wire, seen on Live, against a real doze-aws and the real AWS CLI: a throttle rule makes `aws sqs send-message`
// fail with the SDK's own retries (three attempts, each a tagged row), a slow rule is served and says its time includes the wait,
// and Stop all lets the call through. The rules name this browser's access key, so no other test's calls are bent.
import type { APIRequestContext } from "@playwright/test";
import type { LabMade } from "../../src/api/types/LabMade";
import type { LabStopped } from "../../src/api/types/LabStopped";
import { aws } from "../server";
import { CONSOLE, ENDPOINT, expect, test } from "./fixtures";

const headers = { "X-Console": "1", Origin: CONSOLE };

async function post<T>(request: APIRequestContext, path: string, data: unknown): Promise<T> {
  const answer = await request.post(`${CONSOLE}${path}`, { headers, data });
  expect(answer.ok(), await answer.text()).toBe(true);
  return (await answer.json()) as T;
}

const body = (key: string, operation: string, effect: unknown) => ({
  name: "",
  matcher: { services: ["sqs"], operations: [operation], regions: [], callers: [key], resources: [] },
  effect,
  share: { kind: "every" },
  ends_in_secs: 300,
});

test("a throttle rule fails the AWS CLI with its own retries, Live tags each try, and Stop all lets the call through", async ({ page, request, cli, accessKey }) => {
  const queue = `lab-${accessKey.slice(-6).toLowerCase()}`;
  const made = cli(["sqs", "create-queue", "--queue-name", queue]);
  expect(made.code, made.out).toBe(0);
  const url = (JSON.parse(made.out) as { QueueUrl: string }).QueueUrl;
  try {
    const rule = await post<LabMade>(request, "/api/v1/lab/rules", body(accessKey, "SendMessage", { kind: "fail", error: "throttle" }));
    if (rule.outcome !== "made") throw new Error(JSON.stringify(rule));
    const id = rule.rule.id;

    // The AWS CLI retries a throttle as every SDK does (three attempts in all, its default; the fixture sets one), then gives up with
    // the service's code.
    const refused = aws(ENDPOINT, ["sqs", "send-message", "--queue-url", url, "--message-body", "hi"], { accessKey, attempts: 3 });
    expect(refused.code).not.toBe(0);
    expect(refused.out).toContain("RequestThrottled");
    expect(refused.out).toContain("reached max retries: 2");

    await page.goto("/live");
    const tags = page.locator('[data-slot="lab-tag"]');
    await expect(tags.first()).toBeVisible();
    await expect(tags).toHaveCount(3);
    await expect(tags.first()).toHaveAttribute("data-rule", String(id));
    await expect(tags.first()).toHaveAttribute("data-effect", "failed");
    await expect(page.getByTestId("filter-lab")).toBeVisible();

    // The chip keeps the Lab's calls; the tag says in words what the rule did.
    await page.getByTestId("filter-lab").click();
    await expect(page).toHaveURL(/lab=any/);
    await expect(page.getByTestId("filter-count")).toContainText("3 of");
    await tags.first().hover();
    await expect(page.getByRole("tooltip")).toContainText(`Lab rule ${id}`);
    await expect(page.getByRole("tooltip")).toContainText("never saw it");
    await page.locator("table").last().screenshot({ path: process.env.DOZE_AWS_LAB_SHOTS ? `${process.env.DOZE_AWS_LAB_SHOTS}/live-lab-fail.png` : undefined });

    // The call's drawer says the same, and that the rule is why.
    await page.locator('[data-slot="result"]').first().click();
    await expect(page.getByTestId("call-lab")).toContainText(`Lab rule ${id}`);

    const stopped = await post<LabStopped>(request, "/api/v1/lab/stop-all", {});
    expect(stopped.stopped).toBe(1);
    const sent = cli(["sqs", "send-message", "--queue-url", url, "--message-body", "hi"]);
    expect(sent.code, sent.out).toBe(0);
  } finally {
    await post<LabStopped>(request, "/api/v1/lab/stop-all", {});
    cli(["sqs", "delete-queue", "--queue-url", url]);
  }
});

test("a slow rule serves the call after its wait and the row says its time includes it; a drop is a call with no answer", async ({ page, request, cli, accessKey }) => {
  try {
    const slow = await post<LabMade>(request, "/api/v1/lab/rules", body(accessKey, "ListQueues", { kind: "slow", delay_ms: 400, jitter_ms: 0 }));
    if (slow.outcome !== "made") throw new Error(JSON.stringify(slow));
    const listed = cli(["sqs", "list-queues"]);
    expect(listed.code, listed.out).toBe(0);
    await post<LabStopped>(request, "/api/v1/lab/stop-all", {});

    const drop = await post<LabMade>(request, "/api/v1/lab/rules", { ...body(accessKey, "ListQueues", { kind: "drop" }), matcher: { services: ["sqs"], operations: ["GetQueueUrl"], regions: [], callers: [accessKey], resources: [] } });
    if (drop.outcome !== "made") throw new Error(JSON.stringify(drop));
    // The CLI sees a closed connection, retries it, and gives up with a connection error.
    const closed = cli(["sqs", "get-queue-url", "--queue-name", "nobody"]);
    expect(closed.code).not.toBe(0);
    await post<LabStopped>(request, "/api/v1/lab/stop-all", {});

    await page.goto("/live?lab=any");
    const waited = page.getByTestId("latency-includes");
    await expect(waited).toBeVisible();
    await expect(waited).toContainText("+400");
    await expect(waited).toHaveAttribute("title", /includes \+400 ms/);
    const none = page.locator('[data-slot="result"]', { hasText: "NO ANSWER" });
    await expect(none.first()).toBeVisible();
    await expect(none.first().locator('[data-slot="lab-tag"]')).toHaveAttribute("data-effect", "dropped");
    await page.locator("table").last().screenshot({ path: process.env.DOZE_AWS_LAB_SHOTS ? `${process.env.DOZE_AWS_LAB_SHOTS}/live-lab-slow-drop.png` : undefined });
  } finally {
    await post<LabStopped>(request, "/api/v1/lab/stop-all", {});
  }
});
