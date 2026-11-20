// What IAM did to a call, on Live, against real doze-aws processes: a call `--iam soft` let through that the caller's policies
// refuse is OK with a "would be denied" marker; the same call under `--iam enforce` is DENIED. The caller is an IAM user made
// with the real AWS CLI and given no policy, so IAM refuses everything it asks.
import { aws, start } from "../server";
import { calls, ENDPOINT, expect, openLive, rows, test, unique } from "./fixtures";

/** Makes an IAM user with no policy at `endpoint` and an access key for it: every call signed with the key is refused by IAM. */
function refusedCaller(endpoint: string, name: string): string {
  expect(aws(endpoint, ["iam", "create-user", "--user-name", name]).code).toBe(0);
  const made = aws(endpoint, ["iam", "create-access-key", "--user-name", name]);
  expect(made.code, made.out).toBe(0);
  return (JSON.parse(made.out) as { AccessKey: { AccessKeyId: string } }).AccessKey.AccessKeyId;
}

test("a call soft IAM let through is OK, marked would be denied, and the drawer says why", async ({ page, browserName }) => {
  // The shared doze-aws runs with IAM's default, soft.
  const key = refusedCaller(ENDPOINT, unique("nobody", browserName));
  expect(aws(ENDPOINT, ["sqs", "list-queues"], { accessKey: key }).code).toBe(0);
  await openLive(page);
  const record = (await calls(page)).calls.filter((c) => c.caller === key && c.action === "ListQueues").pop()!;
  expect(record.outcome).toBe("ok");
  expect(record.would_be_denied).toBe(true);
  // The user's own creation, by the account root, was allowed: no marker.
  const made = (await calls(page)).calls.filter((c) => c.action === "CreateAccessKey").pop()!;
  expect(made.would_be_denied).toBe(false);

  const row = page.locator(`tr[data-row="${record.seq}"]`);
  await expect(row.locator('[data-slot="result"]')).toHaveAttribute("data-outcome", "ok");
  const marker = row.locator('[data-slot="would-be-denied"]');
  await expect(marker).toHaveText("would be denied");
  await marker.hover();
  await expect(page.getByRole("tooltip")).toContainText("IAM is soft");
  await expect(page.getByRole("tooltip")).toContainText("--iam enforce");
  await page.mouse.move(0, 0);

  await row.click();
  await expect(page.getByTestId("call-sheet")).toBeVisible();
  await expect(page.getByTestId("call-would-be-denied")).toContainText("let through");
  await expect(page.getByTestId("call-outcome")).toHaveText("ok");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("call-sheet")).toHaveCount(0);
});

test("a call enforced IAM refused is DENIED, told apart from an error", async ({ page, browserName }) => {
  // A doze-aws of this test's own, enforcing, on ports of its own per browser.
  const at = { firefox: 0, webkit: 2, chromium: 4 }[browserName] ?? 6;
  const server = await start(41771 + at, 41772 + at, { args: ["--iam", "enforce"] });
  try {
    const key = refusedCaller(server.endpoint, "nobody");
    const refused = aws(server.endpoint, ["sqs", "list-queues"], { accessKey: key });
    expect(refused.code).not.toBe(0);
    expect(refused.out).toContain("not authorized");
    // An error of the caller's, beside it.
    expect(aws(server.endpoint, ["sqs", "get-queue-url", "--queue-name", "missing"]).code).not.toBe(0);

    await page.goto(`${server.console}/live`);
    await expect(page.getByTestId("live-status")).toHaveText("Live");
    const denied = rows(page).nth(1);
    await expect(denied.locator("td").nth(2)).toHaveText("ListQueues");
    await expect(denied.locator('[data-slot="result"]')).toHaveAttribute("data-outcome", "denied");
    await expect(denied.locator('[data-slot="result"]')).toContainText("DENIED");
    await expect(denied.locator('[data-slot="would-be-denied"]')).toHaveCount(0);
    const error = rows(page).first();
    await expect(error.locator('[data-slot="result"]')).toHaveAttribute("data-outcome", "error");
    // Both are --bad; DENIED alone is outlined and carries a shield.
    const look = (row: typeof error) => row.locator('[data-slot="result"] > span').first().evaluate((el) => getComputedStyle(el).borderTopWidth);
    expect(await look(denied)).toBe("1px");
    expect(await look(error)).toBe("0px");

    await denied.click();
    await expect(page.getByTestId("call-summary")).toContainText("IAM refused it");
    await expect(page.getByTestId("call-outcome")).toHaveText("denied");
  } finally {
    // Off its page first: a page left open on a stopped doze-aws logs the stream's failed reconnects.
    await page.goto("about:blank");
    await server.stop();
  }
});
