// Which policy let a call through, on Live, against a real doze-aws: under `--iam enforce` a call by an IAM user whose group or own
// policy allows it says `allowed by <policy>` in its Result and in the drawer; a call by the account root, one IAM refused, and
// every call under `--iam off` name no policy. The user and its policies are made with the real AWS CLI.
import { LISTEN_PORT } from "../global-setup";
import { aws, start } from "../server";
import { calls, expect, rows, test } from "./fixtures";

/** A user with an access key, and the policy attached; the key. */
function userWith(endpoint: string, name: string, policy: string): string {
  expect(aws(endpoint, ["iam", "create-user", "--user-name", name]).code).toBe(0);
  expect(aws(endpoint, ["iam", "attach-user-policy", "--user-name", name, "--policy-arn", policy]).code).toBe(0);
  const made = aws(endpoint, ["iam", "create-access-key", "--user-name", name]);
  expect(made.code, made.out).toBe(0);
  return (JSON.parse(made.out) as { AccessKey: { AccessKeyId: string } }).AccessKey.AccessKeyId;
}

const POWER = "arn:aws:iam::aws:policy/PowerUserAccess";

test("a call an identity policy allowed says which policy, in Live's Result and in the drawer", async ({ page, browserName }) => {
  const at = { firefox: 0, webkit: 2, chromium: 4 }[browserName] ?? 6;
  const server = await start(LISTEN_PORT + 60 + at, LISTEN_PORT + 61 + at, { args: ["--iam", "enforce"] });
  try {
    const key = userWith(server.endpoint, "dev", POWER);
    // Allowed by PowerUserAccess, as the user; the same call as the account root (no policy is involved); and a user with nothing.
    expect(aws(server.endpoint, ["sqs", "list-queues"], { accessKey: key }).code).toBe(0);
    expect(aws(server.endpoint, ["sqs", "list-queues"]).code).toBe(0);
    const bare = (JSON.parse(aws(server.endpoint, ["iam", "create-user", "--user-name", "bare"]).out) as { User: { UserName: string } }).User.UserName;
    const bareKey = (JSON.parse(aws(server.endpoint, ["iam", "create-access-key", "--user-name", bare]).out) as { AccessKey: { AccessKeyId: string } }).AccessKey.AccessKeyId;
    expect(aws(server.endpoint, ["sqs", "list-queues"], { accessKey: bareKey }).code).not.toBe(0);

    await page.goto(`${server.console}/live`);
    await expect(page.getByTestId("live-status")).toHaveText("Live");
    const record = (await calls(page, server.console)).calls.filter((c) => c.action === "ListQueues");
    const mine = record.find((c) => c.caller === key)!;
    expect(mine.allowed_by).toEqual({ policy: "PowerUserAccess", kind: "aws-managed" });
    expect(record.find((c) => c.caller === "AKIADOZEE2E00000001")!.allowed_by).toBeNull();
    expect(record.find((c) => c.caller === bareKey)!.allowed_by).toBeNull();

    const row = page.locator(`tr[data-row="${mine.seq}"]`);
    await expect(row.getByTestId("allowed-by")).toHaveText(/allowed by PowerUserAccess/);
    await expect(row.locator('[data-slot="result"]')).toHaveAttribute("data-outcome", "ok");
    // The root's call and the refused call say it not.
    await expect(page.locator(`tr[data-row="${record.find((c) => c.caller === bareKey)!.seq}"]`).getByTestId("allowed-by")).toHaveCount(0);
    await expect(rows(page).filter({ has: page.getByTestId("allowed-by") })).toHaveCount(1);

    await row.getByTestId("open-call").click();
    await expect(page.getByTestId("call-sheet")).toBeVisible();
    await expect(page.getByTestId("call-allowed-by")).toContainText("PowerUserAccess");
    await expect(page.getByTestId("call-allowed-by")).toContainText("an AWS managed policy");
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("call-sheet")).toHaveCount(0);
  } finally {
    // Off its page first: a page left open on a stopped doze-aws logs the stream's failed reconnects.
    await page.goto("about:blank");
    await page.goto("about:blank");
    await server.stop();
  }
});

test("under --iam off no call names a policy", async ({ page, browserName }) => {
  const at = { firefox: 0, webkit: 2, chromium: 4 }[browserName] ?? 6;
  const server = await start(LISTEN_PORT + 70 + at, LISTEN_PORT + 71 + at, { args: ["--iam", "off"] });
  try {
    const key = userWith(server.endpoint, "dev", POWER);
    expect(aws(server.endpoint, ["sqs", "list-queues"], { accessKey: key }).code).toBe(0);
    await page.goto(`${server.console}/live`);
    await expect(page.getByTestId("live-status")).toHaveText("Live");
    const record = (await calls(page, server.console)).calls.filter((c) => c.action === "ListQueues" && c.caller === key);
    expect(record).toHaveLength(1);
    expect(record[0]?.allowed_by).toBeNull();
    await expect(page.getByTestId("allowed-by")).toHaveCount(0);
  } finally {
    await page.goto("about:blank");
    await page.goto("about:blank");
    await server.stop();
  }
});
