// The Lab's rule dialog and Send a call, three things that were not there: an error of one's own choosing, picked from what the operation's AWS
// model lists (the code a free-typed word would be is refused in words); a picker for callers (the account's access keys and the callers Live
// has seen); and a text body for S3 PutObject. Each against a doze-aws of the test's own and the real AWS CLI.
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws } from "../server";
import { expect, test, unique } from "./fixtures";
import { addTo, own, sentence } from "./lab-kit";

/** Closes the open picker and waits until it is gone, so the next one is the only one. */
async function closePopover(page: Page): Promise<void> {
  await page.keyboard.press("Escape");
  await expect(page.locator("[cmdk-input]")).toHaveCount(0);
}

/** Chooses the error named `code` in an open dialog: the Error picker, searched, the first thing it offers. */
async function chooseError(page: Page, code: string): Promise<void> {
  await page.getByRole("combobox", { name: "Error", exact: true }).click();
  await page.locator("[cmdk-input]").fill(code);
  await page.keyboard.press("Enter");
  await expect(page.locator("[cmdk-input]")).toHaveCount(0);
}

test("an error of one's own choosing is picked from the operation's model with its status; a code the model does not list is refused in words", async ({ page, accessKey, browserName }) => {
  await own(page, async (server) => {
    const queue = unique("custom", browserName);
    const made = aws(server.endpoint, ["sqs", "create-queue", "--queue-name", queue], { accessKey });
    expect(made.code, made.out).toBe(0);
    const url = (JSON.parse(made.out) as { QueueUrl: string }).QueueUrl;
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("rehearsal-custom").getByTestId("rule-start").click();
    await expect(page.getByTestId("rule-dialog")).toBeVisible();
    await expect(page.getByTestId("rule-kind-custom")).toHaveAttribute("aria-checked", "true");
    await expect(page.getByTestId("rule-kind-custom")).toHaveText("Fail with an error I choose");

    // Nothing to choose from until the service and the operation are named: the list says so.
    await page.getByRole("combobox", { name: "Error", exact: true }).click();
    await expect(page.locator("[cmdk-item]")).toHaveCount(0);
    await closePopover(page);
    await expect(page.locator('[data-field="effect"]')).toContainText("name the service first");
    await addTo(page, "Services", "sqs");
    await addTo(page, "Operations", "SendMessage");
    await addTo(page, "Resources", queue);

    // The list is the model's for SendMessage, each with its status.
    await page.getByRole("combobox", { name: "Error", exact: true }).click();
    await expect(page.locator("[cmdk-item]").filter({ hasText: "InvalidMessageContents" })).toContainText("400");
    await expect(page.locator("[cmdk-item]").filter({ hasText: "QueueDoesNotExist" })).toBeVisible();
    // An error that only another operation lists is not offered.
    await expect(page.locator("[cmdk-item]").filter({ hasText: "OverLimit" })).toHaveCount(0);
    await closePopover(page);

    // A word the model does not list is refused, with the reason, and the rule cannot be made.
    await page.getByRole("combobox", { name: "Error", exact: true }).click();
    await page.locator("[cmdk-input]").fill("Teapot");
    await page.getByTestId("combobox-pattern").click();
    await expect(page.getByTestId("problem-effect")).toContainText("does not list Teapot as an error of SendMessage");
    await expect(page.getByTestId("rule-make")).toBeDisabled();

    // A listed one is made with the model's status, and the sentence says the code.
    await chooseError(page, "InvalidMessageContents");
    await expect(page.getByTestId("problem-effect")).toHaveCount(0);
    await expect.poll(() => sentence(page)).toContain("Answer InvalidMessageContents (400) to");
    await expect(page.getByTestId("rule-sentence")).toContainText("SQS answers 400 InvalidMessageContents");
    // A second operation that does not list it takes the choice back, in words.
    await addTo(page, "Operations", "ReceiveMessage");
    await expect(page.getByTestId("problem-effect")).toContainText("every one of ReceiveMessage and SendMessage");
    await expect(page.getByTestId("rule-make")).toBeDisabled();
    await page.getByRole("combobox", { name: "Operations", exact: true }).click();
    await page.locator("[cmdk-item]").filter({ hasText: "ReceiveMessage" }).click();
    await closePopover(page);
    await expect(page.getByTestId("problem-effect")).toHaveCount(0);
    await page.getByTestId("rule-make").click();
    await expect(page.getByTestId("rule-dialog")).toHaveCount(0);
    const row = page.getByTestId("rule-row");
    await expect(row).toHaveCount(1);
    await expect(row).toContainText("InvalidMessageContents");
    const id = await row.getAttribute("data-rule");

    // The client meets the model's own error: its status and its code, as the CLI prints them.
    const refused = aws(server.endpoint, ["sqs", "send-message", "--queue-url", url, "--message-body", "hi"], { accessKey });
    expect(refused.code).not.toBe(0);
    expect(refused.out).toContain("InvalidMessageContents");
    expect(aws(server.endpoint, ["sqs", "list-queues"], { accessKey }).code).toBe(0);
    // Live says the code on the row the rule touched, and the tag.
    await page.goto(`${server.console}/live?lab=${id}`);
    await expect(page.getByTestId("live-status")).toHaveText("Live");
    const tags = page.locator('[data-slot="lab-tag"]');
    await expect(tags).toHaveCount(1);
    await expect(tags.first()).toHaveAttribute("data-effect", "failed");
    await expect(page.locator('[data-slot="result"]').filter({ has: tags.first() }).first()).toContainText("InvalidMessageContents");
  });
});

test("the callers a rule can name are the account's access keys with their users, and the callers Live has seen; a key in neither is typed in", async ({ page, browserName }) => {
  await own(page, async (server) => {
    const user = unique("lab-app", browserName);
    expect(aws(server.endpoint, ["iam", "create-user", "--user-name", user]).code).toBe(0);
    const key = aws(server.endpoint, ["iam", "create-access-key", "--user-name", user]);
    expect(key.code, key.out).toBe(0);
    const issued = (JSON.parse(key.out) as { AccessKey: { AccessKeyId: string } }).AccessKey.AccessKeyId;
    // An app that signs with a key IAM does not list is a caller Live has seen.
    const seen = `AKIASEEN${browserName.toUpperCase().padEnd(12, "0").slice(0, 12)}`;
    const queue = unique("callers", browserName);
    expect(aws(server.endpoint, ["sqs", "create-queue", "--queue-name", queue], { accessKey: seen }).code).toBe(0);
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("rehearsal-throttle").getByTestId("rule-start").click();
    await page.getByRole("combobox", { name: "Callers", exact: true }).click();
    await expect(page.locator("[cmdk-item]").filter({ hasText: issued })).toContainText(user);
    await expect(page.locator("[cmdk-item]").filter({ hasText: seen })).toBeVisible();
    await closePopover(page);
    // The issued key by search, the seen key, and one that is in neither, as typed.
    await addTo(page, "Callers", issued);
    await addTo(page, "Callers", seen);
    const typed = "AKIATYPEDBYHAND0001";
    await addTo(page, "Callers", typed);
    await expect.poll(async () => { const said = await sentence(page); return [issued, seen, typed].every((k) => said.includes(k)); }).toBe(true);
    // Only the one the rule names is bent: a rule for the seen key alone.
    await page.getByRole("combobox", { name: "Callers", exact: true }).click();
    for (const gone of [issued, typed]) await page.locator("[cmdk-item]").filter({ hasText: gone }).click();
    await closePopover(page);
    await addTo(page, "Services", "sqs");
    await addTo(page, "Operations", "ListQueues");
    await expect.poll(() => sentence(page)).toContain(`from ${seen}`);
    await page.getByTestId("rule-make").click();
    await expect(page.getByTestId("rule-dialog")).toHaveCount(0);
    expect(aws(server.endpoint, ["sqs", "list-queues"], { accessKey: issued }).code).toBe(0);
    const bent = aws(server.endpoint, ["sqs", "list-queues"], { accessKey: seen, attempts: 1 });
    expect(bent.code).not.toBe(0);
    // Its row names the key, with the same words Live uses.
    await expect(page.getByTestId("rule-row")).toContainText("ListQueues");
  });
});

test("Send a call puts a text body for S3 PutObject through the upload's path; too long a body, and UploadPart, are refused in words", async ({ page, accessKey, browserName }) => {
  await own(page, async (server) => {
    const bucket = unique("body", browserName);
    expect(aws(server.endpoint, ["s3api", "create-bucket", "--bucket", bucket], { accessKey }).code).toBe(0);
    await page.goto(`${server.console}/lab?tab=send&api=s3&op=PutObject`);
    await expect(page.getByTestId("lab-body")).toContainText("65,536 bytes");
    await page.getByTestId("lab-input").fill(JSON.stringify({ Bucket: bucket, Key: "notes/hello.txt", ContentType: "text/plain" }));
    await page.getByTestId("lab-body-input").fill("héllo from the Lab\nsecond line");
    await expect(page.getByTestId("lab-body-size")).toContainText("31 of 65,536 bytes");
    await page.getByTestId("lab-send").click();
    await expect(page.getByTestId("lab-answer")).toContainText("Answered");
    await expect(page.getByTestId("lab-output")).toContainText("ETag");
    // The object is there, byte for byte, as the CLI reads it.
    const dir = mkdtempSync(join(tmpdir(), "lab-put-"));
    try {
      const got = aws(server.endpoint, ["s3api", "get-object", "--bucket", bucket, "--key", "notes/hello.txt", join(dir, "out.txt")], { accessKey });
      expect(got.code, got.out).toBe(0);
      expect(readFileSync(join(dir, "out.txt"), "utf8")).toBe("héllo from the Lab\nsecond line");
      expect(got.out).toContain("text/plain");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
    // Too long: said at once, and it cannot be sent.
    await page.getByTestId("lab-body-input").fill("x".repeat(65_537));
    await expect(page.getByTestId("lab-body-help")).toContainText("over 65,536 bytes (64 KiB)");
    await expect(page.getByTestId("lab-send")).toBeDisabled();
    // UploadPart: why, and no body box.
    await page.goto(`${server.console}/lab?tab=send&api=s3&op=UploadPart`);
    await expect(page.getByTestId("lab-cannot")).toContainText("multipart upload");
    await expect(page.getByTestId("lab-body")).toHaveCount(0);
    await expect(page.getByTestId("lab-send")).toBeDisabled();
  });
});

test("a rule on S3 PutObject bends an app's upload and not the Lab's own Send a call (the Lab's calls are the console's, over the bus)", async ({ page, accessKey, browserName }) => {
  await own(page, async (server) => {
    const dir = mkdtempSync(join(tmpdir(), "lab-bent-"));
    const file = join(dir, "app.txt");
    writeFileSync(file, "an app's upload");
    const bucket = unique("bent", browserName);
    expect(aws(server.endpoint, ["s3api", "create-bucket", "--bucket", bucket], { accessKey }).code).toBe(0);
    await page.goto(`${server.console}/lab`);
    // A rule that fails every PutObject (a denial: no SDK retries it).
    const made = await page.evaluate(async () => {
      const res = await fetch("/api/v1/lab/rules", {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-Console": "1" },
        body: JSON.stringify({ matcher: { services: ["s3"], operations: ["PutObject"] }, effect: { kind: "fail", error: "access-denied" }, ends_in_secs: 600 }),
      });
      return (await res.json()) as { outcome: string };
    });
    expect(made.outcome).toBe("made");
    const put = aws(server.endpoint, ["s3api", "put-object", "--bucket", bucket, "--key", "app.txt", "--body", file], { accessKey });
    expect(put.code).not.toBe(0);
    expect(put.out).toContain("AccessDenied");
    await page.goto(`${server.console}/lab?tab=send&api=s3&op=PutObject`);
    await page.getByTestId("lab-input").fill(JSON.stringify({ Bucket: bucket, Key: "lab.txt" }));
    await page.getByTestId("lab-body-input").fill("sent from the Lab");
    await page.getByTestId("lab-send").click();
    // The Lab's call went over the bus, so the rule did not see it: it is answered, and the object is there. The app's upload was refused.
    await expect(page.getByTestId("lab-answer")).toContainText("Answered");
    const got = aws(server.endpoint, ["s3api", "get-object", "--bucket", bucket, "--key", "lab.txt", join(dir, "lab.txt")], { accessKey });
    expect(got.code, got.out).toBe(0);
    expect(readFileSync(join(dir, "lab.txt"), "utf8")).toBe("sent from the Lab");
    const missing = aws(server.endpoint, ["s3api", "head-object", "--bucket", bucket, "--key", "app.txt"], { accessKey });
    expect(missing.code).not.toBe(0);
    rmSync(dir, { recursive: true, force: true });
  });
});
