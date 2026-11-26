// The Lab's page: its two tabs (in the address), and the recipes that name the person's own things. A recipe is tested against a doze-aws
// of its own (an empty one, started here), so "nothing there" and "something there" are known, and the AWS CLI sees what the recipe did.
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aws, ACCOUNT } from "../server";
import { expect, open, overflowX, pickFrom, test, unique } from "./fixtures";
import { own } from "./lab-kit";

test("the tabs are in the address, a reload keeps the tab, and the keyboard goes along them", async ({ page, browserName }) => {
  await open(page, "/lab", "Lab");
  const rehearse = page.getByTestId("lab-tab-rehearse");
  const send = page.getByTestId("lab-tab-send");
  await expect(rehearse).toHaveAttribute("aria-current", "page");
  await expect(send).not.toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("lab-rules")).toBeVisible();
  // The old address of the Lab (an operation named) is the Send a call tab.
  await open(page, "/lab?api=sqs&op=ListQueues", "Lab");
  await expect(send).toHaveAttribute("aria-current", "page");
  await open(page, "/lab", "Lab");
  await rehearse.focus();
  // Safari on macOS does not put a link in the Tab order unless its setting says so (Option+Tab does): there the next tab is focused
  // directly, and Enter is what is under test.
  if (browserName === "webkit") await send.focus();
  else await page.keyboard.press("Tab");
  await expect(send).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/lab\?tab=send/);
  await expect(page.getByTestId("lab-compose")).toBeVisible();
  await page.reload();
  await expect(page.getByTestId("lab-tab-send")).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("lab-compose")).toBeVisible();
  await page.getByTestId("lab-tab-rehearse").click();
  await expect(page).not.toHaveURL(/tab=/);
  await expect(page.getByTestId("lab-tab-rehearse")).toHaveAttribute("aria-current", "page");
});

test("Rehearse has the six rule rehearsals, the error of one's choosing and the five forced events, each with what it stands for in production", async ({ page }) => {
  await open(page, "/lab", "Lab");
  await expect(page.getByText("Not built yet")).toHaveCount(0);
  await expect(page.getByText("Needs a newer doze-aws")).toHaveCount(0);
  await expect(page.getByTestId("rehearsal-cards").getByRole("article")).toHaveCount(7);
  await expect(page.getByTestId("rehearsal-custom")).toContainText("In production:");
  await expect(page.getByTestId("rehearsal-custom").getByTestId("rule-start")).toBeEnabled();
  await expect(page.getByTestId("event-cards").getByRole("article")).toHaveCount(5);
  await expect(page.getByTestId("rehearsal-throttle")).toContainText("In production:");
  await expect(page.getByTestId("rehearsal-throttle").getByTestId("rule-start")).toBeEnabled();
  await expect(page.getByTestId("rehearsal-rest-service")).toContainText("In production:");
  await expect(page.getByTestId("rehearsal-rest-service").getByTestId("once-open")).toBeEnabled();
  // How the Lab works is the server's words, folded.
  await expect(page.getByTestId("lab-intro")).toContainText(/Lab/);
  await expect(page.getByTestId("lab-in-force")).toContainText("No rules in force");
  await expect(page.getByTestId("lab-stop-all")).toBeDisabled();
});

test("Send a call picks a service and an operation, starts from the model's example, and says what an invalid input is", async ({ page }) => {
  await open(page, "/lab?tab=send", "Lab");
  await pickFrom(page, "Service", /^SQS\b/);
  await pickFrom(page, "Operation", "CreateQueue");
  await expect(page).toHaveURL(/api=sqs&op=CreateQueue|op=CreateQueue&api=sqs/);
  const input = page.getByTestId("lab-input");
  await expect(input).toHaveValue(/QueueName/);
  await expect(page.getByTestId("lab-members")).toContainText("input members in the model");
  // Copy as: the AWS CLI line and boto3 are offered for a valid input.
  await expect(page.getByTestId("copy-as")).toBeVisible();
  await input.fill("{ not json");
  await expect(page.getByTestId("lab-input-why")).toBeVisible();
  await expect(page.getByTestId("lab-send")).toBeDisabled();
  await page.getByTestId("lab-reset").click();
  await expect(input).toHaveValue(/QueueName/);
  await expect(page.getByTestId("lab-send")).toBeEnabled();
});

test("S3 PutObject asks for a text body, and UploadPart says in words why it cannot be sent", async ({ page }) => {
  await open(page, "/lab?tab=send&api=s3&op=PutObject", "Lab");
  await expect(page.getByTestId("lab-body")).toContainText("65,536 bytes");
  await expect(page.getByTestId("lab-cannot")).toHaveCount(0);
  await open(page, "/lab?tab=send&api=s3&op=UploadPart", "Lab");
  await expect(page.getByTestId("lab-cannot")).toContainText("CreateMultipartUpload");
  await expect(page.getByTestId("lab-body")).toHaveCount(0);
  await expect(page.getByTestId("lab-send")).toBeDisabled();
});

test("with nothing made, the recipes say to make something first; a recipe appears when its thing exists and goes when it does not", async ({ page, accessKey, browserName }) => {
  await own(page, async (server, cli) => {
    const lab = `${server.console}/lab`;
    await page.goto(lab);
    await expect(page.getByTestId("lab-recipes")).toBeVisible();
    await expect(page.getByTestId("lab-recipe-cards")).toHaveCount(0);
    await expect(page.getByTestId("lab-recipes")).toContainText("Nothing to try yet");
    await expect(page.getByTestId("lab-recipes-resources")).toBeVisible();
    const queue = unique("recipe", browserName);
    expect(aws(server.endpoint, ["sqs", "create-queue", "--queue-name", queue], { accessKey }).code).toBe(0);
    await page.reload();
    const card = page.getByTestId("lab-recipe-sqs-send");
    await expect(card).toBeVisible();
    await expect(card.getByTestId("lab-recipe-title")).toHaveText(`Send a message to ${queue}`);
    // Only what exists: no recipe for a table, a topic or a bucket.
    await expect(page.getByTestId("lab-recipe-dynamodb-put")).toHaveCount(0);
    await expect(page.getByTestId("lab-recipe-sns-publish")).toHaveCount(0);
    expect(cli(["sqs", "delete-queue", "--queue-url", `${server.endpoint}/${ACCOUNT}/${queue}`]).code).toBe(0);
    await page.reload();
    await expect(page.getByTestId("lab-recipe-sqs-send")).toHaveCount(0);
    await expect(page.getByTestId("lab-recipes")).toContainText("Nothing to try yet");
  });
});

test("the queue recipe opens Send a call with the queue's URL, the call is real, and the next step opens the queue", async ({ page, accessKey, browserName }) => {
  await own(page, async (server) => {
    const queue = unique("recsend", browserName);
    const url = `${server.endpoint}/${ACCOUNT}/${queue}`;
    expect(aws(server.endpoint, ["sqs", "create-queue", "--queue-name", queue], { accessKey }).code).toBe(0);
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("lab-recipe-sqs-send").getByTestId("lab-recipe-open").click();
    await expect(page).toHaveURL(/tab=send/);
    await expect(page.getByTestId("lab-note")).toContainText("From the recipe: Send a message to");
    await expect(page.getByTestId("lab-input")).toHaveValue(new RegExp(`"QueueUrl": "${url.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"`));
    await page.getByTestId("lab-send").click();
    await expect(page.getByTestId("lab-answer")).toContainText("Answered");
    await expect(page.getByTestId("lab-output")).toContainText("MessageId");
    // The effect, seen by the AWS CLI.
    const got = aws(server.endpoint, ["sqs", "receive-message", "--queue-url", url], { accessKey });
    expect(got.code).toBe(0);
    expect(got.out).toContain("Hello from the Lab");
    await expect(page.getByTestId("lab-in-live")).toBeVisible();
    // The recipe's next step.
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("lab-recipe-sqs-send").getByTestId("lab-recipe-next").click();
    await expect(page).toHaveURL(new RegExp(`/resources/sqs/queues/${queue}`));
  });
});

test("the table, bucket and secret recipes send real calls; a secret's value is hidden until it is shown", async ({ page, accessKey, browserName }) => {
  await own(page, async (server) => {
    const table = unique("rectable", browserName);
    const bucket = unique("recbucket", browserName);
    const secret = unique("recsecret", browserName);
    const run = (args: string[]) => expect(aws(server.endpoint, args, { accessKey }).code).toBe(0);
    run(["dynamodb", "create-table", "--table-name", table, "--attribute-definitions", "AttributeName=id,AttributeType=S", "--key-schema", "AttributeName=id,KeyType=HASH", "--billing-mode", "PAY_PER_REQUEST"]);
    run(["s3api", "create-bucket", "--bucket", bucket]);
    const dir = mkdtempSync(join(tmpdir(), "lab-body-"));
    writeFileSync(join(dir, "hello.txt"), "hello");
    run(["s3api", "put-object", "--bucket", bucket, "--key", "hello.txt", "--body", join(dir, "hello.txt")]);
    rmSync(dir, { recursive: true, force: true });
    run(["secretsmanager", "create-secret", "--name", secret, "--secret-string", "hush-hush-value"]);
    await page.goto(`${server.console}/lab`);
    // A table: the item is put with the key the table declares.
    await page.getByTestId("lab-recipe-dynamodb-put").getByTestId("lab-recipe-open").click();
    await expect(page.getByTestId("lab-input")).toHaveValue(/"id":/);
    await page.getByTestId("lab-send").click();
    await expect(page.getByTestId("lab-answer")).toContainText("Answered");
    const item = aws(server.endpoint, ["dynamodb", "get-item", "--table-name", table, "--key", '{"id":{"S":"lab-1"}}'], { accessKey });
    expect(item.out).toContain("lab-1");
    // A bucket: it is looked inside.
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("lab-recipe-s3-list").getByTestId("lab-recipe-open").click();
    await page.getByTestId("lab-send").click();
    await expect(page.getByTestId("lab-output")).toContainText("hello.txt");
    // A secret: the answer is there, its value is not until it is shown.
    await page.goto(`${server.console}/lab`);
    await page.getByTestId("lab-recipe-secrets-get").getByTestId("lab-recipe-open").click();
    await page.getByTestId("lab-send").click();
    await expect(page.getByTestId("lab-output")).toContainText("SecretString");
    // The answer carries a long ARN: it scrolls inside its box, the page does not widen.
    expect(await overflowX(page)).toBe(0);
    await expect(page.getByTestId("lab-output")).not.toContainText("hush-hush-value");
    await page.getByTestId("lab-reveal").click();
    await expect(page.getByTestId("lab-output")).toContainText("hush-hush-value");
    await page.getByTestId("lab-reveal").click();
    await expect(page.getByTestId("lab-output")).not.toContainText("hush-hush-value");
  });
});

test("neither tab scrolls sideways at 1440 or at 1024", async ({ page }) => {
  for (const width of [1440, 1024]) {
    await page.setViewportSize({ width, height: 900 });
    for (const path of ["/lab", "/lab?tab=send&api=sqs&op=ListQueues"]) {
      await open(page, path, "Lab");
      if (path.includes("send")) {
        await page.getByTestId("lab-send").click();
        await expect(page.getByTestId("lab-answer")).toBeVisible();
      } else {
        await expect(page.getByTestId("lab-recipes")).toBeVisible();
      }
      expect(await overflowX(page), `${path} at ${width}`).toBe(0);
    }
  }
});
