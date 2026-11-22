// The 22 KMS boards, each reached against a real doze-aws (one per browser, seeded as the boards' "larder" world with the real AWS
// CLI) and, in Chromium, photographed light and dark into SHOTS_DIR.
import { expect, test } from "./fixtures";
import { start, type Running } from "../server";
import { seedKmsetc } from "./kmsetc-seed";
import { cliOf, keyOf, open, portsFor, shoot } from "./kmsetc-kit";
import { expectCrumbsEnd } from "./crumbs-kit";

test.describe.configure({ mode: "serial" });

let server: Running;
let empty: Running;
let browser = "";
let cli: (a: string[]) => string;
const shot = (page: Parameters<typeof shoot>[0], board: string) => shoot(page, browser, board);
const ROLE = (name: string) => `arn:aws:iam::111122223333:role/${name}`;

test.beforeAll(async ({ browserName }) => {
  browser = browserName;
  const [l, c] = portsFor(browserName, 0);
  server = await start(l, c);
  cli = cliOf(server);
  seedKmsetc(server.endpoint, server.account);
  const [l2, c2] = portsFor(browserName, 3);
  empty = await start(l2, c2);
});
test.afterAll(async () => {
  await server.stop();
  await empty.stop();
});

test("kms__keys-list, kms__aws-managed-keys, kms__keys-none: the keys by who manages them, and a list with none", async ({ page }) => {
  await open(page, server.console, "/resources/kms/keys", "Customer managed keys");
  await expect(page.getByTestId("list-sentence")).toContainText("5 customer managed keys: 3 enabled, 1 disabled and 1 pending deletion.");
  await expect(page.getByTestId("list-sentence")).toContainText("alias/larder-app encrypts the queue pick-lists");
  await expect(page.getByRole("table", { name: "Keys" }).locator("tbody tr[data-row]")).toHaveCount(5);
  await expect(page.getByRole("table", { name: "Keys" })).toContainText("larder-app (+1 more)");
  await expect(page.getByRole("table", { name: "Keys" })).toContainText("Pending deletion");
  await page.getByTestId(`select-${keyOf(cli, "larder-legacy")}`).check();
  await expect(page.getByTestId("list-note")).toContainText("larder-legacy is selected and Disabled.");
  await shot(page, "kms__keys-list");
  // AWS managed keys have their own heading: SSM and Secrets Manager make theirs the first time they need one.
  await page.getByTestId("tab-aws").click();
  await expect(page.locator("h1").first()).toHaveText("AWS managed keys");
  await expect(page.getByTestId("list-sentence")).toContainText("AWS managed key, all Enabled. Each alias has the form aws/<service-name>.");
  await expect(page.getByRole("table", { name: "Keys" })).toContainText("aws/secretsmanager");
  await shot(page, "kms__aws-managed-keys");
  await open(page, empty.console, "/resources/kms/keys", "Customer managed keys");
  await expect(page.getByTestId("list-empty")).toContainText("No customer managed keys");
  await expect(page.getByTestId("create-empty")).toBeVisible();
  await shot(page, "kms__keys-none");
});

test("kms__key-schedule-deletion, kms__keys-deletion-scheduled: a waiting period and a confirmation, then the list says when the key goes", async ({ page }) => {
  await open(page, server.console, "/resources/kms/keys", "Customer managed keys");
  const legacy = keyOf(cli, "larder-legacy");
  await page.getByTestId(`select-${legacy}`).check();
  await page.getByTestId("key-actions").click();
  await page.getByTestId("action-schedule").click();
  await expect(page.locator("h1").first()).toHaveText("Schedule key deletion");
  await expect(page.getByTestId("deletion-warning")).toContainText("destructive and potentially dangerous");
  await expect(page.getByRole("table", { name: "Keys to delete" })).toContainText("larder-legacy");
  await expect(page.getByTestId("window-note")).toContainText("7 to 30 days; the API default is 30.");
  await expect(page.getByTestId("window-note")).toContainText("With 7, larder-legacy is deleted on");
  await expect(page.getByTestId("window-note")).not.toContainText("these keys");
  await expect(page.getByTestId("call-panel")).toContainText('"PendingWindowInDays": 7');
  await page.getByTestId("deletion-days").fill("3");
  await expect(page.getByTestId("deletion-ends")).toContainText("Enter a whole number of days from 7 to 30");
  await page.getByTestId("deletion-days").fill("7");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("confirm-problem")).toBeVisible();
  await page.getByTestId("confirm-deletion").click();
  await expectCrumbsEnd(page, ["Schedule key deletion"]);
  await shot(page, "kms__key-schedule-deletion");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("scheduled-note")).toContainText("Cancel key deletion puts larder-legacy back in Disabled");
  await expect(page.getByTestId("list-sentence")).toContainText("larder-legacy changed to Pending deletion and will be deleted on");
  await expect(page.getByTestId("list-sentence")).toContainText("2 pending deletion");
  expect(JSON.parse(cli(["kms", "describe-key", "--key-id", legacy])).KeyMetadata.KeyState).toBe("PendingDeletion");
  await shot(page, "kms__keys-deletion-scheduled");
  // Cancel brings a key back Disabled, never Enabled; Enable is refused for a key pending deletion until then.
  await page.getByTestId(`select-${legacy}`).check();
  await page.getByTestId("key-actions").click();
  await expect(page.getByTestId("action-enable")).toHaveAttribute("data-disabled", "");
  await page.getByTestId("action-cancel").click();
  await expect(page.getByTestId("action-done")).toContainText("larder-legacy is no longer pending deletion");
  await expect(page.getByRole("table", { name: "Keys" }).locator("tr[data-row]", { hasText: "larder-legacy" })).toContainText("Disabled");
});

test("kms__key-create ... kms__key-create-review: six steps, and the key is made with its policy, tags and alias", async ({ page }) => {
  await open(page, server.console, "/resources/kms/keys?create=yes", "Configure key");
  await expect(page.getByTestId("call-panel")).toContainText('"KeySpec": "SYMMETRIC_DEFAULT"');
  await expect(page.getByTestId("section-config")).toContainText("Your other keys: larder-app encrypts and decrypts, larder-legacy");
  await expect(page.getByTestId("origin-external")).toBeDisabled();
  await page.getByTestId("kind-asymmetric").click();
  await expect(page.getByTestId("key-spec")).toBeVisible();
  await page.getByTestId("kind-symmetric").click();
  await shot(page, "kms__key-create");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Add labels");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("field-alias")).toContainText("Enter an alias");
  await page.getByTestId("key-alias").fill("larder-app");
  await expect(page.getByTestId("field-alias")).toContainText("larder-app is already an alias");
  await page.getByTestId("key-alias").fill("larder-reports");
  await page.getByTestId("key-description").fill("Encrypts report-job output");
  await page.getByTestId("key-tags").getByRole("button", { name: "Add tag" }).click();
  await page.getByTestId("key-tags").getByRole("textbox").nth(0).fill("Project");
  await page.getByTestId("key-tags").getByRole("textbox").nth(1).fill("larder");
  await expect(page.getByTestId("call-panel")).toContainText("CreateAlias");
  await shot(page, "kms__key-create-labels");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Define key administrative permissions");
  await page.getByTestId(`select-${ROLE("larder-admin")}`).check();
  await expect(page.getByTestId("admins-statement")).toContainText("Allow access for Key Administrators");
  await expect(page.getByTestId("call-panel")).toContainText("{ 2 statements }");
  await shot(page, "kms__key-create-admins");
  await page.getByTestId("form-submit").click();
  await page.getByTestId(`select-${ROLE("report-job")}`).check();
  await expect(page.getByTestId("users-statement")).toContainText("Allow attachment of persistent resources");
  await expect(page.getByTestId("call-panel")).toContainText("{ 4 statements }");
  await shot(page, "kms__key-create-users");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("policy-json")).toContainText("Enable IAM User Permissions");
  await shot(page, "kms__key-create-policy");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("review-alias")).toHaveText("larder-reports");
  await expect(page.getByTestId("review-note")).toContainText("single-Region key");
  await shot(page, "kms__key-create-review");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("Key larder-reports created.");
  const made = keyOf(cli, "larder-reports");
  const key = JSON.parse(cli(["kms", "describe-key", "--key-id", made])).KeyMetadata;
  expect([key.Description, key.KeyState]).toEqual(["Encrypts report-job output", "Enabled"]);
  expect(JSON.parse(cli(["kms", "get-key-policy", "--key-id", made, "--policy-name", "default"])).Policy).toContain("larder-admin");
  expect(JSON.parse(cli(["kms", "list-resource-tags", "--key-id", made])).Tags).toEqual([{ TagKey: "Project", TagValue: "larder" }]);
});

test("kms__key-detail, kms__key-policy-edit, kms__key-crypto: a key's page, its policy and its configuration", async ({ page }) => {
  const app = keyOf(cli, "larder-app");
  await open(page, server.console, `/resources/kms/keys/${app}`, "larder-app");
  await expect(page.getByTestId("detail-sentence")).toContainText("alias/larder-app is Enabled. Its key material was rotated on demand on");
  await expect(page.getByTestId("detail-sentence")).toContainText("automatic rotation is off.");
  await expect(page.getByTestId("wired-strip")).toContainText("pick-lists, SQS queue");
  await expect(page.getByTestId("wired-strip")).toContainText("larder-queues");
  await expect(page.getByTestId("general-config")).toContainText(app);
  await expect(page.getByTestId("fact-admins")).toContainText("larder-admin");
  await expect(page.getByTestId("fact-users")).toContainText("report-job");
  await expectCrumbsEnd(page, ["Keys",""]);
  await shot(page, "kms__key-detail");
  await page.getByTestId("policy-switch").click();
  await page.getByTestId("policy-edit").click();
  await expect(page.getByTestId("key-policy-edit")).toBeVisible();
  await expect(page.getByTestId("statement-details")).toContainText("Principals");
  await page.getByTestId("statement-select").click();
  await page.getByRole("option", { name: /Allow use of the key/ }).click();
  await expect(page.getByTestId("statement-details")).toContainText("kms:Encrypt");
  await expect(page.getByTestId("call-panel")).toContainText("PutKeyPolicy");
  await expectCrumbsEnd(page, ["Edit key policy"]);
  await shot(page, "kms__key-policy-edit");
  await page.getByTestId("statement-add").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("key-page")).toBeVisible();
  expect(JSON.parse(cli(["kms", "get-key-policy", "--key-id", app, "--policy-name", "default"])).Policy).toContain("Statement5");
  // The policy now has a statement the console did not write: it is shown as the JSON it is.
  await page.getByTestId("tab-policy").click();
  await expect(page.getByTestId("policy-json")).toContainText("Statement5");
  await expect(page.getByTestId("policy-switch")).toHaveCount(0);
  await page.getByTestId("tab-crypto").click();
  await expect(page.getByTestId("crypto-card")).toContainText("SYMMETRIC_DEFAULT");
  await expect(page.getByTestId("crypto-note")).toContainText("256-bit AES-GCM");
  await shot(page, "kms__key-crypto");
  await open(page, server.console, `/resources/kms/keys/${keyOf(cli, "larder-signing")}?tab=crypto`, "larder-signing");
  await expect(page.getByTestId("fact-Signing algorithms")).toContainText("RSASSA");
});

test("kms__key-tags, kms__key-tags-none: tags shown and edited, and a key with none", async ({ page }) => {
  const app = keyOf(cli, "larder-app");
  await open(page, server.console, `/resources/kms/keys/${app}?tab=tags`, "larder-app");
  await expect(page.getByTestId("tags-count")).toHaveText("3 tags");
  await page.getByTestId("tags-edit-open").click();
  // The board: a heading and a line over the rows, Remove and Add tag, and the rules under them.
  await expect(page.getByTestId("tags-edit").getByRole("heading", { name: "Tags" })).toBeVisible();
  await expect(page.getByTestId("tags-lead")).toContainText("Each tag on a key must have a different tag key; the value may be empty.");
  await expect(page.getByTestId("tags-kv").getByRole("button", { name: "Remove" })).toHaveCount(3);
  await expect(page.getByTestId("tags-rules")).toContainText("Removing a tag corresponds to UntagResource");
  await shot(page, "kms__key-tags");
  await page.getByTestId("tags-kv").getByRole("button", { name: "Add tag" }).click();
  await page.getByTestId("tags-kv").getByRole("textbox").nth(6).fill("owner");
  await page.getByTestId("tags-kv").getByRole("textbox").nth(7).fill("ops");
  await page.getByTestId("tags-save").click();
  await expect(page.getByTestId("tags-saved")).toBeVisible();
  expect(JSON.parse(cli(["kms", "list-resource-tags", "--key-id", app])).Tags).toHaveLength(4);
  const bare = JSON.parse(cli(["kms", "list-keys"])).Keys.map((k: { KeyId: string }) => k.KeyId).find((id: string) => !JSON.parse(cli(["kms", "list-aliases", "--key-id", id])).Aliases.length);
  await open(page, server.console, `/resources/kms/keys/${bare}?tab=tags`, bare);
  await expect(page.getByTestId("tags-view")).toContainText("No tags");
  await expect(page.getByTestId("tags-view")).toContainText("This key has no tags.");
  await shot(page, "kms__key-tags-none");
  await page.getByTestId("tab-rotation").click();
  await expect(page.getByTestId("rotations-none")).toContainText("No rotations yet");
  await shot(page, "kms__key-rotations-none");
  await page.getByTestId("tab-aliases").click();
  await expect(page.getByTestId("aliases-none")).toContainText("This key has none, so it is addressed by its key ID or ARN.");
  await shot(page, "kms__key-aliases-none");
});

test("kms__key-rotation, kms__key-rotation-edit: rotation status, Rotate now and the period", async ({ page }) => {
  const app = keyOf(cli, "larder-app");
  await open(page, server.console, `/resources/kms/keys/${app}?tab=rotation`, "larder-app");
  await expect(page.getByTestId("rotation-enabled")).toContainText("false");
  await expect(page.getByTestId("key-materials")).toContainText("On-demand");
  await expect(page.getByTestId("on-demand")).toContainText("1 completed rotation");
  await shot(page, "kms__key-rotation");
  await page.getByTestId("rotation-edit").click();
  await expect(page.getByTestId("rotation-edit-page")).toBeVisible();
  await page.getByTestId("rotation-days").fill("50");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("field-period")).toContainText("90 to 2,560");
  await page.getByTestId("rotation-days").fill("180");
  await expect(page.getByTestId("call-panel")).toContainText('"RotationPeriodInDays": 180');
  await shot(page, "kms__key-rotation-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("rotation-enabled")).toContainText("true");
  expect(JSON.parse(cli(["kms", "get-key-rotation-status", "--key-id", app])).RotationPeriodInDays).toBe(180);
  await page.getByTestId("rotate-now").click();
  await expect(page.getByTestId("rotated-banner")).toBeVisible();
  await expect(page.getByTestId("on-demand")).toContainText("2 completed rotations");
});

test("kms__key-aliases, kms__key-alias-create: the aliases of a key, Create alias from the key and Delete", async ({ page }) => {
  const app = keyOf(cli, "larder-app");
  await open(page, server.console, `/resources/kms/keys/${app}?tab=aliases`, "larder-app");
  await expect(page.getByTestId("alias-count")).toHaveText("2 aliases");
  await page.getByTestId(`select-larder-queues`).check();
  await expect(page.getByTestId("aliases-note")).toContainText("Deleting larder-queues leaves the key untouched");
  await shot(page, "kms__key-aliases");
  await page.getByTestId("alias-create-open").click();
  await page.getByTestId("alias-name").fill("alias/x");
  await page.getByTestId("alias-create-submit").click();
  await expect(page.getByTestId("alias-create")).toContainText("Do not type alias/");
  await page.getByTestId("alias-name").fill("larder-reports-reader");
  await shot(page, "kms__key-alias-create");
  await page.getByTestId("alias-create-submit").click();
  await expect(page.getByTestId("alias-made")).toContainText("alias/larder-reports-reader");
  await expect(page.getByTestId("alias-count")).toHaveText("3 aliases");
  await page.getByTestId("select-larder-reports-reader").check();
  await page.getByTestId("alias-delete-open").click();
  await page.getByTestId("alias-delete-submit").click();
  await expect(page.getByTestId("alias-count")).toHaveText("2 aliases");
  expect(JSON.parse(cli(["kms", "list-aliases", "--key-id", app])).Aliases.map((a: { AliasName: string }) => a.AliasName).sort()).toEqual(["alias/larder-app", "alias/larder-queues"]);
});

test("a key that does not exist has a page of its own that says so", async ({ page, refusals }) => {
  // The service refuses a key that is not there: the page is that refusal.
  refusals.allow(/ GET .*\/resources\/kms\/keys\/00000000-0000-0000-0000-000000000000$/);
  await page.goto(`${server.console}/resources/kms/keys/00000000-0000-0000-0000-000000000000`);
  await expect(page.getByTestId("detail-error")).toBeVisible();
  await expect(page.getByTestId("detail-error")).toContainText("00000000-0000-0000-0000-000000000000");
});
