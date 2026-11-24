// The 17 Secrets Manager boards, each reached against a real doze-aws (one per browser, seeded as the boards' "larder" world with the
// real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR.
import { advance, press, expect, test } from "./fixtures";
import { start, type Running } from "../server";
import { seedKmsetc, STRIPE } from "./kmsetc-seed";
import { cliOf, open, portsFor, shoot } from "./kmsetc-kit";
import { expectCrumbsEnd } from "./crumbs-kit";

test.describe.configure({ mode: "serial" });

let server: Running;
let empty: Running;
let browser = "";
let cli: (a: string[]) => string;
const shot = (page: Parameters<typeof shoot>[0], board: string) => shoot(page, browser, board);
const STRIPE_KEY = "larder%2Fprod%2Fstripe-key";
const page_of = (name: string, q = "") => `/resources/secretsmanager/secrets/${name}${q}`;

test.beforeAll(async ({ browserName }) => {
  browser = browserName;
  const [l, c] = portsFor(browserName, 1);
  server = await start(l, c);
  cli = cliOf(server);
  seedKmsetc(server.endpoint, server.account);
  const [l2, c2] = portsFor(browserName, 4);
  empty = await start(l2, c2);
});
test.afterAll(async () => {
  await server.stop();
  await empty.stop();
});

test("secretsmanager__secrets-list, secrets-none: the secrets, searched by the filters ListSecrets takes, and a list with none", async ({ page }) => {
  // An app reads a secret: the line over the list says who, which and when, from the Live record.
  cli(["secretsmanager", "get-secret-value", "--secret-id", "larder/prod/stripe-key"]);
  await open(page, server.console, "/resources/secretsmanager/secrets", "Secrets");
  await expect(page.getByTestId("list-sentence")).toContainText("2 secrets: 1 with rotation on.");
  await expect(page.getByTestId("list-activity")).toContainText("read larder/prod/stripe-key");
  await expect(page.getByTestId("list-activity")).toContainText("changed today");
  await expect(page.getByRole("table", { name: "Secrets" })).toContainText("larder/prod/stripe-key");
  await expect(page.getByRole("table", { name: "Secrets" })).toContainText("Postgres credentials for the larder application database.");
  // The board: a heading with the count, a search box, a chip for each property, and the filters in use as chips.
  await expect(page.getByTestId("list-heading")).toHaveText("Secrets (2)");
  for (const p of ["name", "description", "tag-key", "tag-value"]) await expect(page.getByTestId(`filter-prop-${p}`)).toBeVisible();
  await page.getByTestId("filter-prop-name").click();
  await page.getByTestId("filter-text").fill("Larder/prod/");
  await page.getByTestId("filter-text").press("Enter");
  await expect(page.getByTestId("list-sentence")).toContainText("0 secrets match of 2.");
  await expect(page.getByText("No secret matches")).toBeVisible();
  await page.getByTestId("filter-remove").click();
  await page.getByTestId("filter-prop-name").click();
  await page.getByTestId("filter-text").fill("larder/prod/");
  await page.getByTestId("filter-text").press("Enter");
  await expect(page.getByRole("table", { name: "Secrets" }).locator("tbody tr[data-row]")).toHaveCount(2);
  await expect(page.getByTestId("filter-chip")).toContainText("Name: larder/prod/");
  await shot(page, "secretsmanager__secrets-list");
  await open(page, empty.console, "/resources/secretsmanager/secrets", "Secrets");
  await expect(page.getByTestId("list-empty")).toContainText("No secrets");
  await expect(page.getByTestId("create-open").first()).toHaveText("Store a new secret");
  await shot(page, "secretsmanager__secrets-none");
});

test("secretsmanager__create-type ... create-review: four steps, and the secret is stored with its tags, key, policy and rotation", async ({ page }) => {
  await open(page, server.console, "/resources/secretsmanager/secrets?create=yes", "Store a new secret");
  await expect(page.getByTestId("type-rds")).toBeDisabled();
  await page.getByTestId("value-rows").getByRole("button", { name: "Add row" }).click();
  await page.getByTestId("value-rows").getByRole("textbox").nth(0).fill("api_key");
  await page.getByTestId("value-rows").getByRole("textbox").nth(1).fill("sk_search_EXAMPLE");
  await page.getByRole("combobox", { name: "Encryption key" }).click();
  await page.getByRole("option", { name: "alias/larder-app" }).click();
  await expect(page.getByTestId("call-panel")).toContainText("(masked)");
  await expect(page.getByTestId("call-panel")).toContainText('"KmsKeyId": "alias/larder-app"');
  await shot(page, "secretsmanager__create-type");
  await advance(page);
  await expect(page.getByTestId("section-name")).toBeVisible();
  await press(page);
  await expect(page.getByTestId("field-name")).toContainText("Enter a name.");
  await page.getByTestId("secret-name").fill("larder/prod/stripe-key");
  await expect(page.getByTestId("field-name")).toContainText("already exists");
  await page.getByTestId("secret-name").fill("larder/prod/search-key");
  await page.getByTestId("secret-description").fill("Search provider API key.");
  await page.getByTestId("secret-tags").getByRole("button", { name: "Add tag" }).click();
  await page.getByTestId("secret-tags").getByRole("textbox").nth(0).fill("app");
  await page.getByTestId("secret-tags").getByRole("textbox").nth(1).fill("search-indexer");
  await expect(page.getByTestId("call-panel")).toContainText("Search provider API key.");
  // The service replicates; the form sets a copy up: a Region typed in, and the call carries AddReplicaRegions.
  await page.getByTestId("section-replicate").getByRole("button").first().click();
  await page.getByTestId("replica-add").click();
  await page.getByTestId("replica-region").fill("eu-west-1");
  await expect(page.getByTestId("call-panel")).toContainText('"AddReplicaRegions"');
  await expect(page.getByTestId("call-panel")).toContainText('"Region": "eu-west-1"');
  await expect(page.getByTestId("section-replicate")).not.toContainText("This form does not set up replicas");
  await shot(page, "secretsmanager__create-configure");
  await advance(page);
  await expect(page.getByTestId("section-rotation")).toBeVisible();
  await expect(page.getByTestId("call-panel")).toContainText("With rotation off, this step adds no call");
  await shot(page, "secretsmanager__create-rotation");
  await advance(page);
  await expect(page.getByTestId("review-name")).toHaveText("larder/prod/search-key");
  await shot(page, "secretsmanager__create-review");
  await advance(page);
  await expect(page.getByTestId("stored-banner")).toContainText("Secret larder/prod/search-key stored.");
  const d = JSON.parse(cli(["secretsmanager", "describe-secret", "--secret-id", "larder/prod/search-key"]));
  expect([d.KmsKeyId, d.Tags]).toEqual(["alias/larder-app", [{ Key: "app", Value: "search-indexer" }]]);
  expect((d.ReplicationStatus as { Region: string }[]).map((r) => r.Region)).toEqual(["eu-west-1"]);
  expect(JSON.parse(cli(["secretsmanager", "get-secret-value", "--secret-id", "larder/prod/search-key"])).SecretString).toBe('{"api_key":"sk_search_EXAMPLE"}');
});

test("secretsmanager__secret-overview, secret-value, secret-value-edit: a secret's page, its value as rows, and a new version", async ({ page }) => {
  await open(page, server.console, page_of(STRIPE_KEY), "larder/prod/stripe-key");
  await expect(page.getByTestId("detail-sentence")).toContainText("rotate-stripe-key last rotated it on");
  await expect(page.getByTestId("wired-strip")).toContainText("Encrypted with");
  await expect(page.getByTestId("wired-strip")).toContainText("alias/larder-app");
  await expect(page.getByTestId("secret-details")).toContainText("Stripe live API key and webhook signing secret used by payments.");
  await expect(page.getByTestId("policy-json")).toContainText("secretsmanager:GetSecretValue");
  await expect(page.getByTestId("sample-text")).toContainText("get_secret_value(SecretId=\"larder/prod/stripe-key\")");
  await expect(page.getByTestId("sample-text")).toContainText(server.endpoint);
  // Sample code is a select with a Copy button, and Retrieve is the card's main button.
  await page.getByTestId("sample-lang").click();
  await page.getByTestId("sample-lang-cli").click();
  await expect(page.getByTestId("sample-text")).toContainText("aws secretsmanager get-secret-value");
  await page.getByTestId("sample-lang").click();
  await page.getByTestId("sample-lang-python").click();
  await expect(page.getByTestId("sample-code").getByRole("button", { name: "Copy" })).toBeVisible();
  await expect(page.getByTestId("secret-value").getByTestId("value-retrieve")).toHaveClass(/bg-primary/);
  await shot(page, "secretsmanager__secret-overview");
  await page.getByTestId("value-retrieve").click();
  await expect(page.getByTestId("value-version")).toContainText("labeled AWSCURRENT");
  await expect(page.getByTestId("secret-value")).toContainText("sk_live_EXAMPLE_a8Fq2x");
  await expect(page.getByTestId("secret-value")).toContainText("whsec_EXAMPLE_k3Lm9p");
  await shot(page, "secretsmanager__secret-value");
  await page.getByTestId("value-edit").click();
  await expect(page.getByTestId("rotation-hint")).toContainText("rotate-stripe-key");
  await expect(page.getByTestId("value-rows").getByRole("textbox").nth(0)).toHaveValue("secret_key");
  await expect(page.getByTestId("value-note")).toContainText("becomes AWSPREVIOUS");
  await page.getByTestId("value-rows").getByRole("textbox").nth(1).fill("sk_live_EXAMPLE_new");
  await expect(page.getByTestId("call-panel")).toContainText("(masked)");
  await expectCrumbsEnd(page, ["Edit secret value"]);
  await shot(page, "secretsmanager__secret-value-edit");
  await advance(page);
  await expect(page.getByTestId("value-saved")).toBeVisible();
  expect(JSON.parse(cli(["secretsmanager", "get-secret-value", "--secret-id", "larder/prod/stripe-key"])).SecretString).toContain("sk_live_EXAMPLE_new");
});

test("secretsmanager__policy-edit: the resource permissions, edited and cleared", async ({ page }) => {
  await open(page, server.console, page_of(STRIPE_KEY), "larder/prod/stripe-key");
  await page.getByTestId("policy-edit-open").click();
  await expect(page.getByTestId("edit-policy")).toContainText("Clear the field and save to delete the policy.");
  await expect(page.getByTestId("edit-policy")).toContainText("larder-admin");
  await shot(page, "secretsmanager__policy-edit");
  await page.getByTestId("policy-editor-json").fill("");
  await expect(page.getByTestId("edit-policy-call")).toContainText("DeleteResourcePolicy");
  await page.getByTestId("edit-policy-save").click();
  await expect(page.getByTestId("policy-none")).toBeVisible();
  expect(JSON.parse(cli(["secretsmanager", "get-resource-policy", "--secret-id", "larder/prod/stripe-key"])).ResourcePolicy).toBeUndefined();
});

test("secretsmanager__secret-rotation, rotation-edit: the rotation configuration, Rotate now and Edit rotation", async ({ page }) => {
  await open(page, server.console, page_of(STRIPE_KEY, "?tab=rotation"), "larder/prod/stripe-key");
  await expect(page.getByTestId("fact-status")).toHaveText("Enabled");
  await expect(page.getByTestId("rotation-config")).toContainText("rate(30 days)");
  await expect(page.getByTestId("rotation-config")).toContainText("Window duration 3h");
  await expect(page.getByTestId("rotation-note")).toContainText("createSecret writes the new value as a version labeled AWSPENDING");
  await shot(page, "secretsmanager__secret-rotation");
  await page.getByTestId("rotation-edit-open").click();
  // The board's two ways to write the schedule: a builder (a number and days or hours), and the expression itself.
  await expect(page.getByTestId("schedule-how-builder")).toHaveAttribute("data-state", "on");
  await expect(page.getByTestId("cadence-n")).toHaveValue("30");
  await expect(page.getByTestId("cadence-expression")).toHaveText("rate(30 days)");
  await page.getByTestId("cadence-n").fill("2");
  await page.getByTestId("cadence-unit").click();
  await page.getByRole("option", { name: "hours" }).click();
  await expect(page.getByRole("alert").filter({ hasText: "every 4 hours" })).toBeVisible();
  await expect(page.getByTestId("edit-rotation-save")).toBeDisabled();
  await page.getByTestId("cadence-unit").click();
  await page.getByRole("option", { name: "days" }).click();
  await page.getByTestId("schedule-how-expression").click();
  await expect(page.getByTestId("rotation-schedule")).toHaveValue("rate(2 days)");
  await page.getByTestId("rotation-schedule").fill("every month");
  await expect(page.getByTestId("edit-rotation-save")).toBeDisabled();
  await page.getByTestId("rotation-schedule").fill("rate(7 days)");
  await page.getByTestId("schedule-how-builder").click();
  await expect(page.getByTestId("cadence-n")).toHaveValue("7");
  await expect(page.getByTestId("edit-rotation-call")).toContainText('"ScheduleExpression": "rate(7 days)"');
  await shot(page, "secretsmanager__rotation-edit");
  await page.getByTestId("edit-rotation-save").click();
  await expect(page.getByTestId("rotation-config")).toContainText("rate(7 days)");
  await page.getByTestId("rotate-now").click();
  await expect(page.getByTestId("rotation-started")).toBeVisible();
  expect(JSON.parse(cli(["secretsmanager", "describe-secret", "--secret-id", "larder/prod/stripe-key"])).RotationRules.ScheduleExpression).toBe("rate(7 days)");
});

test("secretsmanager__secret-versions, secret-tags, secret-tags-none: versions by label, tags shown and a secret with none", async ({ page }) => {
  await open(page, server.console, page_of(STRIPE_KEY, "?tab=versions"), "larder/prod/stripe-key");
  await expect(page.getByTestId("versions-card")).toContainText("AWSCURRENT");
  await expect(page.getByTestId("versions-card")).toContainText("AWSPREVIOUS");
  await expect(page.getByTestId("rollback-cli")).toContainText("update-secret-version-stage");
  await expect(page.getByTestId("rollback-cli")).toContainText(server.endpoint);
  await shot(page, "secretsmanager__secret-versions");
  await page.getByTestId("tab-tags").click();
  await expect(page.getByTestId("tags-heading")).toHaveText("Tags (3)");
  await expect(page.getByTestId("tags-count")).toHaveText("Key/value labels you attach to the secret.");
  await expect(page.getByRole("table", { name: "Tags" })).toContainText("payments");
  await expect(page.getByTestId("tags-rules")).toContainText("can't start with the reserved prefix aws:");
  await shot(page, "secretsmanager__secret-tags");
  await open(page, server.console, page_of("larder%2Fprod%2Fdb-credentials", "?tab=tags"), "larder/prod/db-credentials");
  await expect(page.getByTestId("tags-view")).toContainText("This secret has no tags.");
  await shot(page, "secretsmanager__secret-tags-none");
});

test("secretsmanager__secret-delete, secret-deleted: a waiting period, then the secret says it is scheduled and Cancel deletion brings it back", async ({ page }) => {
  await open(page, server.console, page_of(STRIPE_KEY), "larder/prod/stripe-key");
  await page.getByTestId("secret-actions").click();
  await page.getByTestId("action-delete").click();
  await expect(page.getByTestId("delete-secret")).toContainText("Disable secret and schedule deletion");
  await page.getByTestId("deletion-days").fill("3");
  await expect(page.getByTestId("delete-secret-save")).toBeDisabled();
  await page.getByTestId("deletion-days").fill("30");
  await expect(page.getByTestId("delete-secret-call")).toContainText('"RecoveryWindowInDays": 30');
  await shot(page, "secretsmanager__secret-delete");
  await page.getByTestId("delete-secret-save").click();
  await expect(page.getByTestId("secret-scheduled")).toContainText("Scheduled for deletion: the recovery window ends on");
  // The board's reduced page: its state alone beside the name, Cancel deletion as the header's main button, and no strip, tabs or menus.
  await expect(page.getByText("Rotation enabled")).toHaveCount(0);
  await expect(page.getByTestId("wired-strip")).toHaveCount(0);
  await expect(page.getByTestId("tab-overview")).toHaveCount(0);
  await expect(page.getByTestId("actions-menu")).toHaveCount(0);
  await expect(page.getByTestId("secret-actions")).toHaveCount(0);
  await expect(page.getByTestId("cancel-deletion")).toBeVisible();
  await expect(page.getByTestId("secret-scheduled").getByTestId("cancel-deletion")).toHaveCount(0);
  await expect(page.getByTestId("value-scheduled")).toContainText("You can't retrieve the value of a secret that is scheduled for deletion.");
  await expect(page.getByTestId("detail-sentence")).toContainText("is scheduled for deletion");
  await shot(page, "secretsmanager__secret-deleted");
  // After a reload the window's end is not known: the page says what DescribeSecret says.
  await page.goto(`${server.console}${page_of(STRIPE_KEY)}`);
  await expect(page.getByTestId("secret-scheduled")).toContainText("The deletion was scheduled on");
  await open(page, server.console, "/resources/secretsmanager/secrets", "Secrets");
  await expect(page.getByRole("table", { name: "Secrets" })).not.toContainText("stripe-key");
  await page.getByTestId("preferences").click();
  await page.getByTestId("show-deleted").click();
  await expect(page.getByRole("table", { name: "Secrets" })).toContainText("Scheduled for deletion");
  await page.goto(`${server.console}${page_of(STRIPE_KEY)}`);
  await page.getByTestId("cancel-deletion").click();
  await expect(page.getByTestId("restored-banner")).toBeVisible();
  expect(JSON.parse(cli(["secretsmanager", "describe-secret", "--secret-id", "larder/prod/stripe-key"])).DeletedDate).toBeUndefined();
  expect(STRIPE.length).toBeGreaterThan(0);
});

test("a secret that does not exist has a page of its own that says so", async ({ page, refusals }) => {
  // The service refuses a secret that is not there: the page is that refusal.
  refusals.allow(/ GET .*\/resources\/secretsmanager\/secrets\/nope%2Fnothing$/);
  await page.goto(`${server.console}${page_of("nope%2Fnothing")}`);
  await expect(page.getByTestId("detail-error")).toContainText("nope/nothing");
});
