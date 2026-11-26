// Access home, the Resources index, and the direction boards (Main, Spaces, Patterns, System, Scope): what each one says is real
// in the console, reached against a real doze-aws whose apps have called, photographed light and dark in Chromium.
import { expect, test } from "./fixtures";
import { go, serveWorld, shot, world } from "./home-kit";

serveWorld(2);

test("access-home: Identity and access holds IAM and STS, and the rest of Access is under it", async ({ page }) => {
  await go(page, "/access", "Access");
  await expect(page.getByTestId("identity")).toContainText("Identity and access");
  await expect(page.getByTestId("access-iam")).toContainText("Users, roles, groups, policies and access keys");
  await expect(page.getByTestId("access-sts")).toContainText("Who a caller is");
  // What was refused, and who each key is, stay on the page.
  await expect(page.getByTestId("denials")).toContainText("report-job");
  await shot(page, "access-home");
  await page.getByTestId("access-sts").click();
  await expect(page).toHaveURL(/\/resources\/sts$/);
  await go(page, "/access", "Access");
  await page.getByTestId("access-iam").click();
  await expect(page).toHaveURL(/\/resources\/iam$/);
});

test("resources-index: every service by what it is for, with what it holds", async ({ page }) => {
  await go(page, "/resources", "Resources");
  const index = page.getByTestId("resources-index");
  for (const group of ["Messaging and events", "Storage and data", "Compute and flow", "Config and secrets", "Observability", "Infrastructure"]) await expect(index.getByRole("heading", { name: group })).toBeVisible();
  await expect(page.getByTestId("count-sqs")).toContainText("6 queues");
  await expect(page.getByTestId("count-s3")).toContainText("3 buckets");
  await shot(page, "resources-index");
});

test("Spaces: pinned and recent in the rail, resources not services, and ⌘K does as well as goes", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists", "pick-lists");
  await page.getByTestId("pin-toggle").click();
  await expect(page.getByTestId("rail-pinned").getByTestId("rail-place")).toHaveText(["pick-lists"]);
  await go(page, "/resources/s3/buckets/product-images", "product-images");
  await page.getByTestId("pin-toggle").click();
  await expect(page.getByTestId("rail-pinned").getByTestId("rail-place")).toHaveText(["pick-lists", "product-images"]);
  // Opening another resource puts it under Recent; a pinned one is not repeated there.
  await go(page, "/resources/lambda/functions/resize-product-image", "resize-product-image");
  await expect(page.getByTestId("rail-recent").getByTestId("rail-place")).toHaveText(["resize-product-image"]);
  await expect(page.getByTestId("rail-place").filter({ hasText: "pick-lists" })).toHaveCount(1);
  // A pinned place opens its page; Unpin takes it out.
  await page.getByTestId("rail-pinned").getByText("pick-lists").click();
  await expect(page).toHaveURL(/\/resources\/sqs\/queues\/pick-lists$/);
  // Still there after a reload (kept in this browser).
  await page.reload();
  await expect(page.getByTestId("rail-pinned").getByTestId("rail-place")).toHaveCount(2);
  // ⌘K: an action, not only a place.
  await page.getByTestId("palette-trigger").click();
  await page.getByTestId("palette-input").fill("send a message to pick-lists");
  await expect(page.getByTestId("palette-list").getByRole("group", { name: "Do" }).getByRole("option")).toHaveText(["Send a message to pick-lists", "Send a message to pick-lists-dlq"]);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/resources\/sqs\/queues\/pick-lists\?view=messages/);
  await page.getByTestId("palette-trigger").click();
  await page.getByTestId("palette-input").fill("copy the endpoint");
  await expect(page.getByTestId("palette-list").getByRole("option")).toHaveText([/Copy the endpoint/]);
  await page.keyboard.press("Escape");
  await go(page, "/resources/sqs/queues/pick-lists", "pick-lists");
  await shot(page, "Spaces");
  await page.getByTestId("pin-toggle").click();
  await expect(page.getByTestId("rail-pinned").getByTestId("rail-place")).toHaveText(["product-images"]);
});

test("Patterns: a resource page says where you are, what state it is in, what you can do, and what just happened", async ({ page }) => {
  await go(page, "/resources/sqs/queues/pick-lists", "pick-lists");
  await expect(page.getByTestId("detail-sentence")).toContainText("waiting");
  await expect(page.getByTestId("wired-strip")).toBeVisible();
  for (const verb of ["edit", "delete", "purge", "messages"]) await expect(page.getByTestId(`verb-${verb}`)).toBeVisible();
  await expect(page.getByTestId("recent-calls")).toBeVisible();
  await shot(page, "Patterns");
});

test("System: a service is a descriptor: its list says its columns and a sentence, from the service's own state", async ({ page }) => {
  await go(page, "/resources/sqs/queues", "Queues");
  await expect(page.getByTestId("list-sentence")).toContainText("hold");
  await expect(page.getByRole("table", { name: "Queues" }).locator("tbody tr[data-row]")).toHaveCount(6);
  await shot(page, "System");
});

test("Scope: what doze-aws cannot do is said in a form, on the resource, on the wire, and in Needs a look", async ({ page }) => {
  // 1. In a form: the options that cannot work stay, disabled, with why.
  await go(page, "/resources/sns/subscriptions?create=yes", /Create subscription/);
  const list = page.getByTestId("protocol-list");
  await expect(list.getByRole("radio")).toHaveCount(9);
  for (const off of ["email", "email-json", "sms", "application", "firehose"]) await expect(list.getByTestId(`protocol-radio-${off}`)).toBeDisabled();
  for (const on of ["http", "https", "sqs", "lambda"]) await expect(list.getByTestId(`protocol-radio-${on}`)).toBeEnabled();
  await expect(list.getByTestId("protocol-email")).toContainText("it would need a mail server");
  await expect(list.getByTestId("protocol-sms")).toContainText("SMS needs a carrier");
  await expect(list.getByTestId("protocol-firehose")).toContainText("Amazon Data Firehose, which doze-aws does not have");
  // 2. On the resource: the subscription is stored and says so.
  await go(page, "/resources/sns/subscriptions", "Subscriptions");
  await expect(page.getByTestId("not-active-banner")).toContainText("1 subscription was created but is not active locally.");
  await expect(page.getByTestId("not-active")).toContainText("Not active locally. Stored. doze-aws has no mail server, so nothing is sent.");
  // The whole of it is on the page: the text ends inside the cell it is in.
  const cell = await page.getByTestId("sub-status-cell").last().boundingBox();
  const said = await page.getByTestId("not-active").boundingBox();
  expect(said!.x + said!.width).toBeLessThanOrEqual(cell!.x + cell!.width + 1);
  // Why? says it for the protocol.
  await page.getByTestId("not-active-why").click();
  await expect(page.getByTestId("not-active-reasons")).toContainText("email: Stored. doze-aws has no mail server");
  await expect(page.getByTestId("not-active-reasons")).toContainText("pending confirmation");
  await shot(page, "Scope");
  // 3. On the wire: a call to what is not built, with its words.
  await go(page, "/live?outcome=not-built", "Live");
  const row = page.locator("tr[data-row]", { hasText: "SelectObjectContent" }).first();
  await expect(row).toContainText("NOT BUILT");
  await row.getByTestId("open-call").click();
  await expect(page.getByTestId("call-summary")).toContainText("doze-aws does not do this yet: the operation is not built.");
  await expect(page.getByTestId("not-built")).toContainText("SelectObjectContent is not built in doze-aws");
  await expect(page.getByTestId("not-built")).toContainText("X-Doze-Not-Built: 1");
  await expect(page.getByTestId("not-built-instead")).toContainText("GetObject returns the whole object");
  await expect(page.getByTestId("not-built-open")).toContainText("reports");
  await expect(page.getByTestId("not-built-copy")).toBeVisible();
  await page.keyboard.press("Escape");
  await go(page, "/live", "Live");
  await expect(page.locator('[data-testid="look"][data-word="NOT ACTIVE LOCALLY"]').filter({ hasText: "1 subscription stored" })).toHaveCount(1);
  await expect(page.locator('[data-testid="look"][data-word="NOT BUILT"]')).toContainText("SelectObjectContent");
  // 4. Where it surfaces on its own: the count in Needs a look on Home.
  await go(page, "/", "Home");
  await expect(page.getByTestId("needs-a-look")).toContainText("NOT BUILT");
  await expect(page.getByTestId("needs-a-look")).toContainText("Not active locally");
  await expect(page.getByTestId("needs-a-look")).toContainText("1 subscription stored");
});

test("an empty place says what is missing and offers the Create button, and nothing more", async ({ page }) => {
  await go(page, "/resources/kinesis/streams", "Data streams");
  await expect(page.getByTestId("list-empty")).toContainText("No data streams");
  await expect(page.getByTestId("list-empty").getByRole("button", { name: "Create data stream" })).toBeVisible();
  await expect(page.getByTestId("from-your-app")).toHaveCount(0);
  await expect(page.getByText("Or from your app")).toHaveCount(0);
  await shot(page, "Patterns.empty");
  await go(page, "/resources/secretsmanager/secrets", "Secrets");
  await expect(page.getByTestId("list-empty")).toContainText("No secrets");
  await expect(page.getByTestId("from-app-cli")).toHaveCount(0);
});

test("Main: the five jobs each have a place: connect, watch, build, understand, rehearse", async ({ page }) => {
  await go(page, "/", "Home");
  await expect(page.getByTestId("home-connect")).toBeVisible();
  for (const space of ["live", "resources", "access", "lab", "settings"]) await expect(page.getByTestId(`space-${space}`)).toBeVisible();
  await page.getByTestId("space-lab").click();
  await expect(page.locator("h1").first()).toHaveText("Lab");
  await go(page, "/", "Home");
  await shot(page, "Main");
  expect(world.apps.payments).toMatch(/^AKIA/);
});
