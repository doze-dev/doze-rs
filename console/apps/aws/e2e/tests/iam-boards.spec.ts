// The 35 IAM and STS boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder" world with the
// real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board. The flows run in the order the
// boards tell them; the deletes come after the boards they would empty, and the three "none left" lists are read from a second, empty doze-aws.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { seedLarder, trust, type World } from "./iam-seed";
import { CUT, expect, test } from "./fixtures";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-iam");
const BASE = 45100;
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });
// Several boards are IAM's refusal (a duplicate, a malformed document): the browser logs each 4xx as a failed load.
test.use({ allowErrors: CUT });

let server: Running;
let empty: Running | null = null;
let world: World;
let browserKind = "";
const cli = (args: string[], key?: string) => {
  const r = aws(server.endpoint, args, { accessKey: key });
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

/** Photographs the page as the board is: light, then dark (the class the theme switch sets), in Chromium only. */
async function shot(page: Page, board: string) {
  if (browserKind !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
  const height = await page.evaluate(() => Math.max(document.documentElement.scrollHeight, window.innerHeight));
  await page.setViewportSize({ width: 1440, height });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(SHOTS, `${board}.light.png`) });
  await page.evaluate(() => document.documentElement.classList.add("dark"));
  await page.waitForTimeout(250);
  await page.screenshot({ path: join(SHOTS, `${board}.dark.png`) });
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await page.setViewportSize({ width: 1440, height: 900 });
}

const go = async (page: Page, path: string, title: string | RegExp, base = () => server.console) => {
  await page.goto(`${base()}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
};
const iam = (kind: string, name = "", q = "") => `/resources/iam/${kind}${name ? `/${encodeURIComponent(name)}` : ""}${q}`;
const policyArn = (name: string) => `arn:aws:iam::${server.account}:policy/${name}`;

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  world = seedLarder(server.endpoint, server.account);
});
test.afterAll(async () => {
  await empty?.stop();
  await server.stop();
});

test("iam__dashboard and iam__alias-edit: the counts, the account, and the alias form with its rule and its calls", async ({ page }) => {
  await go(page, "/resources/iam", "IAM Dashboard");
  await expect(page.getByTestId("dashboard-sentence")).toContainText("larder-dev has 4 users, 2 groups, 4 roles and 2 customer managed policies.");
  await expect(page.getByTestId("count-users-n")).toHaveText("4 in this account");
  await expect(page.getByTestId("count-policies")).toContainText("customer managed");
  await expect(page.getByTestId("count-providers")).toContainText("0 OpenID Connect and 0 SAML in this account");
  await expect(page.getByTestId("fact-Account Alias")).toContainText("larder-dev");
  await expect(page.getByTestId("alias-note")).toContainText("only one alias");
  await shot(page, "iam__dashboard");
  await page.getByTestId("alias-edit").click();
  await expect(page.locator("h1").first()).toHaveText("Account alias");
  await expect(page.getByTestId("field-alias")).toContainText("3 to 63 characters");
  await expect(page.getByTestId("field-alias")).toContainText("overwrites larder-dev");
  await shot(page, "iam__alias-edit");
  // The rule is said before any call; a valid new alias deletes the old and makes the new.
  await page.getByTestId("alias-input").fill("Larder");
  await expect(page.getByTestId("field-alias")).toContainText("only lowercase letters");
  await expect(page.getByTestId("form-submit")).toBeDisabled();
  await page.getByTestId("alias-input").fill("larder-dev-2");
  await expect(page.getByTestId("call-panel")).toContainText("DeleteAccountAlias");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("IAM Dashboard");
  await expect(page.getByTestId("fact-Account Alias")).toContainText("larder-dev-2");
  expect(JSON.parse(cli(["iam", "list-account-aliases"])).AccountAliases).toEqual(["larder-dev-2"]);
  cli(["iam", "delete-account-alias", "--account-alias", "larder-dev-2"]);
  cli(["iam", "create-account-alias", "--account-alias", "larder-dev"]);
});

test("iam__users-list and iam__user-delete: the keys by user, the sentence, and the dialog that names what goes", async ({ page }) => {
  await go(page, iam("users"), "Users");
  await expect(page.getByTestId("list-sentence")).toContainText("4 users: ci-bot and dev each hold 1 active access key and both used it today; temp-auditor and tester have no access keys.");
  await expect(page.getByRole("row", { name: /dev .*Active - AKIA/ })).toBeVisible();
  await expect(page.getByTestId("page-line")).toHaveText("1 of 1 page, 4 users");
  await expect(page.getByTestId("action-delete")).toBeDisabled();
  await page.getByTestId("select-temp-auditor").click();
  await expect(page.getByTestId("action-delete")).toBeEnabled();
  await shot(page, "iam__users-list");
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("This permanently deletes temp-auditor. Once an IAM user and its access keys are deleted, they cannot be restored or recovered.");
  await expect(page.getByTestId("delete-keys")).toContainText("No access keys. temp-auditor has none, so nothing signs requests as this user.");
  await expect(page.getByTestId("delete-how")).toContainText("DeleteUser answers DeleteConflict");
  await expect(page.getByTestId("delete-dialog-submit")).toBeDisabled();
  await page.getByTestId("delete-dialog-input").fill("temp-auditor");
  await expect(page.getByTestId("delete-dialog-submit")).toBeEnabled();
  await shot(page, "iam__user-delete");
  await page.getByTestId("delete-dialog-input").fill("temp-auditor");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("User temp-auditor deleted.");
  await expect(page.getByTestId("list-sentence")).toContainText("3 users");
  cli(["iam", "create-user", "--user-name", "temp-auditor"]);
});

test("iam__user-detail, user-credentials, user-policies-none and access-keys-none: the user, its keys and what it may do", async ({ page }) => {
  await go(page, iam("users", "dev"), "dev");
  await expect(page.getByTestId("detail-sentence")).toContainText("dev belongs to 1 group and gets PowerUserAccess through it. Its key AKIA");
  await expect(page.getByTestId("detail-sentence")).toContainText("with sqs in us-east-1");
  await expect(page.getByTestId("wired-strip")).toContainText("developers group");
  await expect(page.getByTestId("permissions-card")).toContainText("PowerUserAccess");
  await expect(page.getByTestId("permissions-card")).toContainText("Group developers");
  await expect(page.getByTestId("permissions-note")).toContainText("Nothing is attached to dev directly and it has no inline policy, so removing dev from developers would leave it with no permissions.");
  await expect(page.getByTestId("boundary-card")).toContainText("Not set.");
  await expect(page.getByTestId("tab-groups")).toContainText("1");
  await shot(page, "iam__user-detail");
  await page.getByTestId("tab-security").click();
  await expect(page.getByTestId("keys-card")).toContainText("Access keys (1)");
  await expect(page.getByTestId("keys-card")).toContainText(world.dev);
  await expect(page.getByTestId("keys-card")).toContainText("sqs, us-east-1");
  await expect(page.getByTestId("rotate-note")).toContainText("and so does doze-aws");
  await shot(page, "iam__user-credentials");
  await go(page, iam("users", "tester"), "tester");
  await expect(page.getByTestId("permissions-none")).toContainText("tester has no policies attached and belongs to no group, so it is allowed to do nothing.");
  await shot(page, "iam__user-policies-none");
  await page.getByTestId("tab-security").click();
  await expect(page.getByTestId("keys-none")).toContainText("tester has no access keys, so it cannot call the API from code. A user can hold up to two.");
  await shot(page, "iam__access-keys-none");
  // A user that is not there is said, not a red box.
  await page.goto(`${server.console}${iam("users", "nobody")}`);
  await expect(page.getByTestId("not-there")).toContainText("No user named nobody");
});

test("iam__access-key-retrieve: the key is made with a description tag and its secret is shown once", async ({ page }) => {
  await go(page, iam("users", "dev", "?tab=security"), "dev");
  await page.getByTestId("create-key").click();
  await expect(page.locator("h1").first()).toHaveText("Access key best practices & alternatives");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Set description tag");
  await page.getByTestId("key-description").fill("dev-laptop");
  await expect(page.getByTestId("call-panel")).toContainText("TagUser");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Retrieve access keys");
  await expect(page.getByTestId("key-made")).toContainText("The key pair is active by default");
  await expect(page.getByTestId("made-secret")).toContainText("****");
  const id = (await page.getByTestId("made-id").textContent()) ?? "";
  expect(id).toMatch(/^AKIA/);
  await expect(page.getByTestId("call-panel")).toContainText(id);
  await shot(page, "iam__access-key-retrieve");
  await page.getByTestId("secret-toggle").click();
  await expect(page.getByTestId("made-secret")).not.toContainText("****");
  await page.getByTestId("key-done").click();
  await expect(page.getByTestId("keys-card")).toContainText("Access keys (2)");
  await expect(page.getByTestId("create-key")).toBeDisabled();
  const tags = JSON.parse(cli(["iam", "list-user-tags", "--user-name", "dev"])).Tags as { Key: string; Value: string }[];
  expect(tags).toEqual([{ Key: id, Value: "dev-laptop" }]);
  // The new key is real: a call signed with it is dev's.
  const used = JSON.parse(cli(["iam", "get-access-key-last-used", "--access-key-id", id])).UserName;
  expect(used).toBe("dev");
  cli(["iam", "delete-access-key", "--user-name", "dev", "--access-key-id", id]);
});

test("iam__user-create, user-create-permissions and user-create-review: three steps, nothing made until the last", async ({ page }) => {
  await go(page, iam("users", "", "?create=yes"), "Specify user details");
  await expect(page.getByTestId("field-name")).toContainText("ci-bot, dev, temp-auditor and tester already exist.");
  await page.getByTestId("user-name").fill("dev-intern");
  await expect(page.getByTestId("call-panel")).toContainText('"UserName": "dev-intern"');
  await expect(page.getByTestId("console-access")).toContainText("Not available in doze-aws");
  await shot(page, "iam__user-create");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Set permissions");
  await expect(page.getByTestId("mode-words")).toContainText("in soft mode, the default, doze-aws records the would-be denial and lets the call through");
  await page.getByTestId("select-developers").click();
  await expect(page.getByTestId("call-panel")).toContainText("AddUserToGroup");
  await expect(page.getByTestId("call-panel")).toContainText("dev-intern inherits PowerUserAccess through developers");
  await shot(page, "iam__user-create-permissions");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Review and create");
  await expect(page.getByTestId("review-list")).toContainText("developers");
  expect(aws(server.endpoint, ["iam", "get-user", "--user-name", "dev-intern"]).code).not.toBe(0);
  await shot(page, "iam__user-create-review");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("User dev-intern created.");
  await page.getByTestId("view-created").click();
  await expect(page.locator("h1").first()).toHaveText("dev-intern");
  expect(JSON.parse(cli(["iam", "list-groups-for-user", "--user-name", "dev-intern"])).Groups[0].GroupName).toBe("developers");
});

test("iam__groups-list, group-detail, group-members-none and group-create: the groups, their members, and the form that makes one", async ({ page }) => {
  await go(page, iam("groups"), "User groups");
  await expect(page.getByTestId("list-sentence")).toContainText("2 groups: developers has 2 members, dev and dev-intern, and one attached policy, PowerUserAccess; contractors has no members and no attached policy.");
  await expect(page.getByRole("row", { name: /developers 2/ })).toBeVisible();
  await shot(page, "iam__groups-list");
  await go(page, iam("groups", "developers"), "developers");
  await expect(page.getByTestId("detail-sentence")).toContainText("developers has 2 members and 1 attached policy. Everyone in it can do what PowerUserAccess allows: every service except IAM, Organizations and Account.");
  await expect(page.getByTestId("members-card")).toContainText("Users in this group (2)");
  await expect(page.getByTestId("recent-calls")).toContainText("AddUserToGroup");
  await shot(page, "iam__group-detail");
  await go(page, iam("groups", "contractors"), "contractors");
  await expect(page.getByTestId("members-none")).toContainText("contractors has no members. Add users to give them the group's permissions.");
  await shot(page, "iam__group-members-none");
  await go(page, iam("groups", "", "?create=yes"), "Create user group");
  await page.getByTestId("group-name").fill("support");
  await expect(page.getByTestId("field-name")).toContainText("contractors and developers already exist.");
  await page.getByTestId("select-dev").click();
  await page.getByTestId("select-arn:aws:iam::aws:policy/AmazonSQSReadOnlyAccess").click();
  await expect(page.getByTestId("call-panel")).toContainText("AttachGroupPolicy");
  await shot(page, "iam__group-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("User group support created.");
  const g = JSON.parse(cli(["iam", "get-group", "--group-name", "support"]));
  expect(g.Users.map((u: { UserName: string }) => u.UserName)).toEqual(["dev"]);
  expect(JSON.parse(cli(["iam", "list-attached-group-policies", "--group-name", "support"])).AttachedPolicies[0].PolicyName).toBe("AmazonSQSReadOnlyAccess");
});

test("iam__roles-list, role-detail, role-trust and role-policies-none: the roles, what each may do, and who may assume it", async ({ page }) => {
  await go(page, iam("roles"), "Roles");
  await expect(page.getByTestId("list-sentence")).toContainText("4 roles. report-job was assumed 2 times in the last hour, each time with ci-bot's key");
  await expect(page.getByRole("row", { name: /report-job AWS account: / })).toBeVisible();
  await expect(page.getByRole("row", { name: /lambda-exec AWS service: lambda/ })).toBeVisible();
  await shot(page, "iam__roles-list");
  await go(page, iam("roles", "report-job"), "report-job");
  await expect(page.getByTestId("detail-sentence")).toContainText("report-job was assumed 2 times in the last hour, all with ci-bot's key. Its only permission is s3:GetObject on reports/*, through report-job-readonly.");
  await expect(page.getByTestId("wired-strip")).toContainText("reports bucket");
  await expect(page.getByTestId("fact-Maximum session duration")).toHaveText("1 hour");
  await expect(page.getByTestId("permissions-card")).toContainText("report-job-readonly");
  await expect(page.getByTestId("session-note")).toContainText("AssumeRole refuses a DurationSeconds above this role's");
  await expect(page.getByTestId("recent-calls")).toContainText("AssumeRole");
  await shot(page, "iam__role-detail");
  await page.getByTestId("tab-trust").click();
  await expect(page.getByTestId("detail-sentence")).toContainText("report-job names two principals in this account, ci-bot and dev, and one action, sts:AssumeRole.");
  await expect(page.getByTestId("trust-json")).toContainText("// the role's trust policy");
  await expect(page.getByTestId("trust-note")).toContainText("Stored, not applied locally.");
  await shot(page, "iam__role-trust");
  await go(page, iam("roles", "dispatch-role"), "dispatch-role");
  await expect(page.getByTestId("permissions-none")).toContainText("dispatch-role has no policies attached, so it is allowed to do nothing.");
  await shot(page, "iam__role-policies-none");
});

test("iam__role-trust-edit: the trust policy is checked as it is typed and replaced whole", async ({ page }) => {
  await go(page, iam("roles", "report-job", "?tab=trust"), "report-job");
  await page.getByTestId("edit-trust").click();
  await expect(page.locator("h1").first()).toHaveText("Edit trust policy");
  await expect(page.getByTestId("policy-json-status")).toHaveText("No syntax errors");
  await shot(page, "iam__role-trust-edit");
  // A Resource in a trust policy is refused before the call; a Principal added is kept.
  const base = { Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { AWS: [`arn:aws:iam::${server.account}:user/ci-bot`, `arn:aws:iam::${server.account}:user/dev`, `arn:aws:iam::${server.account}:user/dev-intern`] }, Action: "sts:AssumeRole" }] };
  await page.getByTestId("policy-json-editor").fill(JSON.stringify({ ...base, Statement: [{ ...base.Statement[0], Resource: "*" }] }));
  await expect(page.getByTestId("policy-json-errors")).toContainText("names a Resource, which a trust policy cannot have");
  await expect(page.getByTestId("form-submit")).toBeDisabled();
  await page.getByTestId("policy-json-editor").fill(JSON.stringify(base, null, 2));
  await expect(page.getByTestId("call-panel")).toContainText("UpdateAssumeRolePolicy");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("trust-updated")).toContainText("The trust policy of report-job was updated.");
  await expect(page.getByTestId("detail-sentence")).toContainText("names three principals in this account, ci-bot, dev and dev-intern");
  const doc = JSON.parse(cli(["iam", "get-role", "--role-name", "report-job"])).Role.AssumeRolePolicyDocument;
  expect(doc.Statement[0].Principal.AWS).toHaveLength(3);
  cli(["iam", "update-assume-role-policy", "--role-name", "report-job", "--policy-document", trust({ AWS: [`arn:aws:iam::${server.account}:user/ci-bot`, `arn:aws:iam::${server.account}:user/dev`] })]);
});

test("iam__role-create, role-create-permissions and role-create-review: the trust policy it builds, the policies, and the review", async ({ page }) => {
  await go(page, iam("roles", "", "?create=yes"), "Select trusted entity");
  await expect(page.getByTestId("trust-json")).toContainText(`arn:aws:iam::${server.account}:root`);
  await expect(page.getByTestId("trust-panel")).toContainText("Stored, not applied locally.");
  await shot(page, "iam__role-create");
  await page.getByTestId("trust-external").click();
  await page.getByTestId("trust-external-id").fill("larder-ext");
  await expect(page.getByTestId("trust-json")).toContainText("sts:ExternalId");
  await page.getByTestId("trust-external").click();
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Add permissions");
  await page.getByTestId("select-arn:aws:iam::aws:policy/AmazonSQSReadOnlyAccess").click();
  await page.getByTestId("select-arn:aws:iam::aws:policy/AmazonS3ReadOnlyAccess").click();
  await expect(page.getByTestId("policy-note")).toContainText("By default a role has no permissions.");
  await expect(page.getByRole("row", { name: /report-job-readonly Customer managed 1/ })).toBeVisible();
  await shot(page, "iam__role-create-permissions");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Name, review, and create");
  await page.getByTestId("role-name").fill("larder-readonly");
  await page.getByTestId("role-description").fill("Read-only look at the larder queues and buckets.");
  await expect(page.getByTestId("field-name")).toContainText("already exist");
  await expect(page.getByTestId("review-policies")).toContainText("AmazonSQSReadOnlyAccess");
  await expect(page.getByTestId("review-trust")).toContainText("sts:AssumeRole");
  await shot(page, "iam__role-create-review");
  expect(aws(server.endpoint, ["iam", "get-role", "--role-name", "larder-readonly"]).code).not.toBe(0);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("Role larder-readonly created.");
  const held = JSON.parse(cli(["iam", "list-attached-role-policies", "--role-name", "larder-readonly"])).AttachedPolicies.map((p: { PolicyName: string }) => p.PolicyName).sort();
  expect(held).toEqual(["AmazonS3ReadOnlyAccess", "AmazonSQSReadOnlyAccess"]);
});

test("iam__role-delete: the dialog names what is detached, and the role goes with it", async ({ page }) => {
  await go(page, iam("roles"), "Roles");
  await page.getByTestId("select-larder-admin").click();
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-dialog")).toContainText("This deletes larder-admin.");
  await expect(page.getByTestId("delete-group-Attached policies")).toContainText("1 policy will be detached. AdministratorAccess is attached to larder-admin; the policy itself stays.");
  await expect(page.getByTestId("delete-how")).toContainText("DeleteRole answers DeleteConflict while a managed policy is still attached.");
  await page.getByTestId("delete-dialog-input").fill("larder-admin");
  await shot(page, "iam__role-delete");
  await page.getByTestId("delete-dialog-input").fill("larder-admin");
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Role larder-admin deleted.");
  expect(aws(server.endpoint, ["iam", "get-role", "--role-name", "larder-admin"]).code).not.toBe(0);
});

test("iam__policies-list, policy-detail: the policies, the filter, what each is attached to, and one policy's document", async ({ page }) => {
  await go(page, iam("policies"), "Policies");
  await expect(page.getByTestId("list-sentence")).toContainText("2 customer managed policies are attached: larder-queues-rw to the user ci-bot, report-job-readonly to the role report-job. AWS managed policies are not counted here.");
  await expect(page.getByRole("row", { name: /report-job-readonly Customer managed 1 Read objects/ })).toBeVisible();
  await expect(page.getByTestId("page-line")).toHaveText("1 of 1 page, 2 policies");
  await shot(page, "iam__policies-list");
  // The AWS managed filter lists what something holds.
  await page.getByTestId("policy-scope-aws").click();
  await expect(page.getByRole("row", { name: /PowerUserAccess AWS managed/ })).toBeVisible();
  await expect(page.getByTestId("aws-note")).toContainText("IAM lists an AWS managed policy once something has it attached");
  await page.getByTestId("policy-scope-aws").click();
  await go(page, iam("policies", policyArn("report-job-readonly")), "report-job-readonly");
  await expect(page.getByTestId("detail-sentence")).toContainText("report-job-readonly is attached to 1 role, report-job. Its default version is v1; the policy has not been edited since it was created.");
  await expect(page.getByTestId("wired-strip")).toContainText("reports bucket");
  await expect(page.getByTestId("policy-json")).toContainText('"Action": "s3:GetObject"');
  await expect(page.getByTestId("recent-calls")).toContainText("AttachRolePolicy");
  await shot(page, "iam__policy-detail");
  await page.getByTestId("policy-view-summary").click();
  await expect(page.getByTestId("summary-card")).toContainText("s3:GetObject");
  await page.getByTestId("tab-entities").click();
  await expect(page.getByTestId("entities-card")).toContainText("report-job");
  await page.getByTestId("tab-versions").click();
  await expect(page.getByTestId("versions-card")).toContainText("v1");
});

test("iam__policy-create, policy-create-error and policy-create-review: the editor stops a document that breaks the grammar, then the review", async ({ page }) => {
  await go(page, iam("policies", "", "?create=yes"), "Create policy");
  await expect(page.getByTestId("policy-json-status")).toHaveText("No syntax errors");
  await expect(page.getByTestId("call-panel")).toContainText("(next step)");
  await shot(page, "iam__policy-create");
  const bad = { Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: ["sqs:SendMessage"] }] };
  await page.getByTestId("policy-json-editor").fill(JSON.stringify(bad, null, 2));
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("must-fix")).toHaveText("You must fix the problem before you can continue.");
  await expect(page.getByTestId("policy-json-status")).toHaveText("1 error");
  await expect(page.getByTestId("policy-json-errors")).toContainText("Policy statement must contain resources.");
  await expect(page.getByTestId("resource-hint")).toContainText("MalformedPolicyDocument");
  await expect(page.getByTestId("resource-hint")).toContainText(`arn:aws:sqs:us-east-1:${server.account}:pick-lists`);
  await expect(page.getByTestId("nothing-created")).toContainText("CreatePolicy only runs on the review step");
  await shot(page, "iam__policy-create-error");
  await page.getByTestId("policy-json-editor").fill(JSON.stringify({ ...bad, Statement: [{ ...bad.Statement[0], Resource: `arn:aws:sqs:us-east-1:${server.account}:pick-lists` }] }, null, 2));
  await expect(page.getByTestId("policy-json-status")).toHaveText("No syntax errors");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Create policy");
  await expect(page.getByTestId("section-permissions")).toContainText("sqs:SendMessage");
  await page.getByTestId("policy-name").fill("pick-lists-send-only");
  await page.getByTestId("policy-description").fill("Send messages to pick-lists.");
  await expect(page.getByTestId("field-name")).toContainText("already exist");
  await expect(page.getByTestId("v1-note")).toContainText("version v1");
  await shot(page, "iam__policy-create-review");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("Policy pick-lists-send-only created.");
  const made = JSON.parse(cli(["iam", "get-policy", "--policy-arn", policyArn("pick-lists-send-only")])).Policy;
  expect(made.DefaultVersionId).toBe("v1");
  expect(made.Description).toBe("Send messages to pick-lists.");
});

test("sts__switch-role and sts__role-session: AssumeRole through the form, and the identity menu that holds the session", async ({ page }) => {
  await page.goto(`${server.console}/resources/sts?account=larder-dev&roleName=report-job`);
  await expect(page.locator("h1").first()).toHaveText("Switch Role");
  await expect(page.getByTestId("last-assumed")).toContainText("The role report-job was last assumed at");
  await expect(page.getByTestId("last-assumed")).toContainText("ci-bot's key");
  await expect(page.getByTestId("field-account")).toContainText("larder-dev is the alias of");
  await expect(page.getByTestId("switch-display")).toHaveValue("report-job @ larder-dev");
  await expect(page.getByTestId("call-panel")).toContainText("arn:aws:iam::" + server.account + ":role/report-job");
  await shot(page, "sts__switch-role");
  await page.getByTestId("switch-color-green").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("identity-sheet")).toBeVisible();
  await expect(page.getByTestId("identity-name")).toHaveText("report-job @ larder-dev");
  await expect(page.getByTestId("identity-credentials")).toContainText("AWS_SESSION_TOKEN=");
  await expect(page.getByTestId("identity-recent")).toContainText("report-job @ larder-dev");
  await shot(page, "sts__role-session");
  // Switch back ends the session: the menu closes and the chip is the person's own again.
  await page.getByTestId("switch-back").click();
  await expect(page.getByTestId("identity-sheet")).toHaveCount(0);
  await expect(page.getByTestId("chip-identity")).toContainText("Switch role");
  // A role that needs an external ID is refused before the call.
  cli(["iam", "create-role", "--role-name", "partner-role", "--assume-role-policy-document", JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { AWS: `arn:aws:iam::${server.account}:root` }, Action: "sts:AssumeRole", Condition: { StringEquals: { "sts:ExternalId": "abc" } } }] })]);
  await page.goto(`${server.console}/resources/sts?account=larder-dev&roleName=partner-role`);
  await expect(page.getByTestId("field-role")).toContainText("requires an external ID");
});

test("iam__users-none, groups-none and policies-none: the lists with nothing left, from an empty doze-aws", async ({ page }) => {
  empty = await start(at(browserKind) + 2, at(browserKind) + 3);
  const base = () => (empty as Running).console;
  await go(page, iam("users"), "Users", base);
  await expect(page.getByTestId("list-empty")).toContainText("There are no users in this account. Requests use the account root until you create one.");
  await shot(page, "iam__users-none");
  await go(page, iam("groups"), "User groups", base);
  await expect(page.getByTestId("list-empty")).toContainText("A group lets you attach one set of permissions to many users.");
  await shot(page, "iam__groups-none");
  await go(page, iam("policies"), "Policies", base);
  await expect(page.getByTestId("list-empty")).toContainText("No customer managed policies");
  await shot(page, "iam__policies-none");
});
