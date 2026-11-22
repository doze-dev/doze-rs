// The 31 CloudFormation boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder" world
// with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board. The flows run in the
// order the boards tell them: the stacks and their tabs, the change set, the exports; the create wizard (a change set first, which leaves a stack
// waiting for review, then its execution, which fails on a name that is taken and rolls back); the update wizard on larder-core; a delete that
// ends in the deleted stack's page; and last every stack deleted, which leaves the empty lists.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { Page } from "@playwright/test";
import { aws, start, type Running } from "../server";
import { expect, expectNoErrorAnswers, test } from "./fixtures";
import { seedLarder, settled } from "./cloudformation-seed";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-cloudformation");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45300);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });

let server: Running;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};
const STACKS = "/resources/cloudformation/stacks";
const stack = (name: string, q = "") => `${STACKS}/${encodeURIComponent(name)}${q}`;
const SEARCH_URL = "https://reports.s3.us-east-1.amazonaws.com/templates/larder-search.yaml";
const CORE_V2_URL = "https://reports.s3.us-east-1.amazonaws.com/templates/larder-core-v2.yaml";

/** Photographs the page as the board is: light, then dark (the class the theme switch sets), in Chromium only. */
async function shot(page: Page, board: string) {
  if (browserKind !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(250);
  const height = await page.evaluate(() => Math.max(document.documentElement.scrollHeight, window.innerHeight));
  await page.setViewportSize({ width: 1440, height });
  await page.waitForTimeout(150);
  await page.screenshot({ path: join(SHOTS, `cloudformation__${board}.light.png`) });
  await page.evaluate(() => document.documentElement.classList.add("dark"));
  await page.waitForTimeout(250);
  await page.screenshot({ path: join(SHOTS, `cloudformation__${board}.dark.png`) });
  await page.evaluate(() => document.documentElement.classList.remove("dark"));
  await page.setViewportSize({ width: 1440, height: 900 });
}

const go = async (page: Page, path: string, title: string | RegExp) => {
  await page.goto(`${server.console}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
};
const tab = async (page: Page, name: string, id: string) => {
  await go(page, stack(name), name);
  await page.getByTestId(`tab-${id}`).click();
};

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  await seedLarder(server.endpoint, server.account);
});
test.afterAll(async () => {
  await server.stop();
});

test("cloudformation__stacks-list: the sentence, the nested stack marked, the filter by status", async ({ page }) => {
  await go(page, STACKS, "Stacks");
  await expect(page.getByTestId("list-sentence")).toContainText("8 stacks in us-east-1, one of them nested. larder-reports-job is waiting for its change set to be executed; larder-nightly and larder-hotfix rolled back.");
  await expect(page.getByTestId("nested-marker")).toHaveCount(1);
  await expect(page.getByTestId("cfn-nav")).toContainText("Navigation pane");
  await expect(page.getByTestId("create-open")).toBeVisible();
  await expect(page.getByRole("row")).toHaveCount(9);
  await shot(page, "stacks-list");
  // The filter: a group of statuses, with how many each holds; Deleted reads the records CloudFormation keeps.
  await page.getByTestId("filter-status").click();
  await expect(page.getByTestId("filter-ROLLBACK")).toContainText("2");
  await page.getByTestId("filter-ROLLBACK").click();
  await expect(page.getByRole("row")).toHaveCount(3);
  await expect(page).toHaveURL(/q=ROLLBACK/);
  await expect(page.getByRole("menu")).toHaveCount(0);
  await page.getByTestId("filter-status").click();
  await page.getByTestId("filter-DELETED").click();
  await expect(page.getByTestId("deleted-note")).toContainText("by name it is gone");
});

test("cloudformation__stack-detail: the sentence, what it owns, the verbs, the overview and what each setting does", async ({ page }) => {
  await go(page, stack("larder-core"), "larder-core");
  await expect(page.getByTestId("stack-sentence")).toContainText("larder-core owns 6 resources in 5 services and has been stable since");
  await expect(page.getByTestId("stack-sentence")).toContainText("No stack imports its two exports, so nothing stops you deleting it.");
  await expect(page.getByTestId("wired-strip")).toContainText("Owns");
  await expect(page.getByTestId("wired-strip").getByTestId("wired-link")).toHaveCount(6);
  await expect(page.getByTestId("verb-update")).toBeVisible();
  await expect(page.getByTestId("verb-delete")).toBeVisible();
  await expect(page.getByTestId("detail-badge")).toHaveText("CREATE_COMPLETE");
  for (const t of ["info", "events", "resources", "outputs", "parameters", "template", "changesets"]) await expect(page.getByTestId(`tab-${t}`)).toBeVisible();
  await expect(page.getByTestId("field-Stack ID")).toContainText(":stack/larder-core/");
  await expect(page.getByTestId("tags-block")).toContainText("app: larder");
  await expect(page.getByTestId("tags-block")).toContainText("Applied locally.");
  await expect(page.getByTestId("policy-block")).toContainText("No stack policy is set");
  await expect(page.getByTestId("rollback-block")).toContainText("Not kept in doze-aws.");
  await expect(page.getByTestId("notify-block")).toContainText("arn:aws:sns:us-east-1:111122223333:alerts");
  await expect(page.getByTestId("recent-calls")).toContainText("CreateStack");
  await shot(page, "stack-detail");
  // A resource it owns opens its own page.
  await page.getByTestId("wired-strip").getByRole("link", { name: "pick-lists", exact: true }).click();
  await expect(page.locator("h1").first()).toHaveText("pick-lists");
});

test("cloudformation__stack-detail: termination protection is turned on from Actions, and a delete is then refused in words", async ({ page }) => {
  await go(page, stack("larder-sandbox"), "larder-sandbox");
  await page.getByTestId("actions-menu").click();
  await page.getByRole("menuitem", { name: "Turn on termination protection" }).click();
  await expect(page.getByTestId("protection-dialog")).toContainText("will fail and leave the stack as it is");
  await page.getByTestId("protection-dialog-submit").click();
  await expect(async () => {
    await page.getByTestId("tab-info").click();
    await expect(page.getByTestId("field-Termination protection")).toHaveText("Enabled", { timeout: 1000 });
  }).toPass({ timeout: 10_000 });
  await page.getByTestId("verb-delete").click();
  await expect(page.getByTestId("delete-refusal")).toContainText("Termination protection is on for larder-sandbox");
  await expect(page.getByTestId("delete-stack-submit")).toBeDisabled();
  await page.keyboard.press("Escape");
  await page.getByTestId("actions-menu").click();
  await page.getByRole("menuitem", { name: "Turn off termination protection" }).click();
  await page.getByTestId("protection-dialog-submit").click();
  await expect(async () => {
    await expect(page.getByTestId("field-Termination protection")).toHaveText("Disabled", { timeout: 1000 });
  }).toPass({ timeout: 10_000 });
});

test("cloudformation__stack-events: newest first, the whole history of a stack that was only created", async ({ page }) => {
  await tab(page, "larder-core", "events");
  const rows = page.getByTestId("stack-events").locator("tbody tr");
  await expect(rows.first()).toContainText("larder-core");
  await expect(rows.first()).toContainText("CREATE_COMPLETE");
  await expect(rows.last()).toContainText("User Initiated");
  await expect(page.getByTestId("stack-events")).toContainText("This stack has only been created, so this is its whole history.");
  await expect(page.getByTestId("stack-events")).toContainText("dependency order");
  await shot(page, "stack-events");
});

test("cloudformation__stack-resources: what it owns, with the ones its template retains named", async ({ page }) => {
  await tab(page, "larder-core", "resources");
  await expect(page.getByTestId("stack-resources")).toContainText("except ProductImagesBucket, which has DeletionPolicy Retain");
  await expect(page.getByTestId("stack-resources").locator("tbody tr")).toHaveCount(6);
  await expect(page.getByTestId("stack-resources")).toContainText("AWS::SQS::Queue");
  await expect(page.getByTestId("stack-resources").getByRole("link", { name: "product-images" })).toBeVisible();
  await shot(page, "stack-resources");
});

test("cloudformation__stack-outputs: key, value, description and export, and who imports them", async ({ page }) => {
  await tab(page, "larder-core", "outputs");
  const out = page.getByTestId("stack-outputs");
  await expect(out.locator("tbody tr")).toHaveCount(3);
  await expect(out).toContainText("larder-core-OrdersTopicArn");
  await expect(out).toContainText("Not exported");
  await expect(out).toContainText("No stack imports these 2 today.");
  await shot(page, "stack-outputs");
});

test("cloudformation__stack-parameters: the values it was created with, a NoEcho one masked and explained", async ({ page }) => {
  await tab(page, "larder-core", "parameters");
  const p = page.getByTestId("stack-parameters");
  await expect(p.locator("tbody tr")).toHaveCount(5);
  await expect(p.locator("tr", { hasText: "WebhookSecret" })).toContainText("****");
  await expect(p).toContainText("WebhookSecret is declared with NoEcho");
  await expect(p).toContainText("Use existing value");
  await shot(page, "stack-parameters");
});

test("cloudformation__stack-template: the template as sent, and the processed stage on request", async ({ page }) => {
  await tab(page, "larder-core", "template");
  await expect(page.getByTestId("template-body")).toContainText("AWS::SQS::Queue");
  await expect(page.getByTestId("template-body")).toContainText("DeletionPolicy: Retain");
  await shot(page, "stack-template");
  await page.getByTestId("template-stage-Processed").click();
  await expect(page.getByTestId("stack-template")).toContainText("The template after CloudFormation expanded its transforms");
  await expect(page.getByTestId("template-body")).toContainText("ProductImagesBucket");
});

test("cloudformation__stack-changesets: each set a link, a failed one explained", async ({ page }) => {
  await tab(page, "larder-core", "changesets");
  const t = page.getByTestId("stack-changesets");
  await expect(t.locator("tbody tr")).toHaveCount(2);
  await expect(t).toContainText("add-reports-bucket");
  await expect(t.locator("tr", { hasText: "same-as-deployed" })).toContainText("FAILED");
  await expect(t.locator("tr", { hasText: "same-as-deployed" })).toContainText("UNAVAILABLE");
  await expect(t).toContainText("The submitted information didn't contain changes.");
  await expect(t).toContainText("A change set with status FAILED can't be executed");
  await expect(page.getByTestId("changeset-create")).toBeVisible();
  await shot(page, "stack-changesets");
});

test("cloudformation__outputs-none, parameters-none, changesets-none: a stack that has none says so", async ({ page }) => {
  await tab(page, "larder-sandbox", "outputs");
  await expect(page.getByTestId("outputs-none")).toContainText("This stack's template declares no outputs.");
  await shot(page, "outputs-none");
  await page.getByTestId("tab-parameters").click();
  await expect(page.getByTestId("parameters-none")).toContainText("This stack's template declares no parameters.");
  await shot(page, "parameters-none");
  await page.getByTestId("tab-changesets").click();
  await expect(page.getByTestId("changesets-none")).toContainText("A change set previews an update before you run it.");
  await expect(page.getByTestId("changeset-create")).toBeVisible();
  await shot(page, "changesets-none");
});

test("cloudformation__changeset-detail: what it previews, execute and delete", async ({ page, refusals }) => {
  // A change set that is not there is asked for, and refused.
  refusals.allow(/404 POST .*\/cloudformation\/stacks\/op\/DescribeChangeSet$/);
  await tab(page, "larder-core", "changesets");
  await page.getByTestId("changeset-link").first().click();
  await expect(page.locator("h1").first()).toHaveText("add-reports-bucket");
  await expect(page.getByTestId("changeset-page")).toContainText("It previews 2 changes and has not changed anything yet.");
  await expect(page.getByTestId("changes").locator("tbody tr")).toHaveCount(2);
  await expect(page.getByTestId("changes").locator("tr", { hasText: "ReportsBucket" })).toContainText("Add");
  await expect(page.getByTestId("changes").locator("tr", { hasText: "PickListsQueue" })).toContainText("Modify");
  await expect(page.getByTestId("changeset-parameters")).toContainText("WebhookSecret");
  await expect(page.getByTestId("changeset-parameters")).toContainText("****");
  await expect(page.getByTestId("executes")).toContainText("UPDATE_ROLLBACK_COMPLETE");
  await expect(page.getByTestId("execute-changeset")).toBeEnabled();
  await shot(page, "changeset-detail");
  // A change set that is not there is a page that says so.
  await go(page, stack("larder-core", "?do=changeset&name=nope"), "Change set not found");
  await expect(page.getByTestId("detail-error")).toContainText("There is no change set named nope");
});

test("cloudformation__exports and export-detail: who imports what", async ({ page }) => {
  await go(page, "/resources/cloudformation/exports", "Exports");
  await expect(page.getByTestId("list-sentence")).toContainText("4 exports from 3 stacks. Only one is imported: larder-edge imports larder-observability-DlqAlarmArn, so larder-observability can't be deleted while it does.");
  await expect(page.getByTestId("cfn-nav")).toBeVisible();
  await expect(page.getByRole("row")).toHaveCount(5);
  await expect(page.getByTestId("use-export")).toContainText("Fn::ImportValue");
  await shot(page, "exports");
  await page.getByTestId("export-link").filter({ hasText: "larder-observability-DlqAlarmArn" }).click();
  await expect(page.locator("h1").first()).toHaveText("larder-observability-DlqAlarmArn");
  await expect(page.getByTestId("export-sentence")).toContainText("1 stack imports this export: larder-edge.");
  await expect(page.getByTestId("importers")).toContainText("larder-edge");
  await expect(page.getByTestId("importers")).toContainText("DeleteStack on the exporting stack is cancelled while any import remains");
  await shot(page, "export-detail");
  await go(page, "/resources/cloudformation/exports/nope", "Export not found");
  // A stack that has an import to wait for cannot be deleted: its dialog says so.
  await go(page, stack("larder-observability"), "larder-observability");
  await page.getByTestId("verb-delete").click();
  await expect(page.getByTestId("delete-refusal")).toContainText("larder-edge imports larder-observability-DlqAlarmArn");
  await expect(page.getByTestId("delete-stack-submit")).toBeDisabled();
  await page.keyboard.press("Escape");
});

test("cloudformation__stack-review-pending: a stack waiting for its change set", async ({ page }) => {
  await go(page, stack("larder-reports-job"), "larder-reports-job");
  await expect(page.getByTestId("stack-sentence")).toContainText("larder-reports-job is waiting for review. Its change set first-look would add 1 resource: Queue (SQS queue).");
  await expect(page.getByTestId("stack-sentence")).not.toContainText("being worked on");
  await expect(page.getByTestId("review-note")).toContainText("Nothing is deployed yet.");
  await expect(page.getByTestId("verb-execute")).toBeVisible();
  await expect(page.getByTestId("verb-dropset")).toBeVisible();
  await expect(page.getByTestId("changes").locator("tbody tr")).toHaveCount(1);
  await expect(page.getByTestId("changeset-overview")).toContainText("first-look");
  await expect(page.getByTestId("executes")).toContainText("Executing builds the stack");
  await shot(page, "stack-review-pending");
});

test("cloudformation__stack-rolled-back: a create that failed holds nothing, and only a delete can be done", async ({ page }) => {
  await go(page, stack("larder-nightly"), "larder-nightly");
  await expect(page.getByTestId("detail-badge")).toHaveText("ROLLBACK_COMPLETE");
  await expect(page.getByTestId("stack-sentence")).toContainText("failed to create and CloudFormation undid it. Taken could not be made");
  await expect(page.getByTestId("stack-sentence")).toContainText("It holds no resources.");
  await expect(page.getByTestId("stack-sentence")).not.toContainText("held before");
  await expect(page.getByTestId("rolled-note")).toContainText("Only a delete can be performed on this stack.");
  await expect(page.getByTestId("verb-delete")).toBeVisible();
  await expect(page.getByTestId("verb-update")).toHaveCount(0);
  await expect(page.getByTestId("stack-events")).toContainText("CREATE_FAILED");
  await expect(page.getByTestId("stack-events")).toContainText("already exists");
  await expect(page.getByTestId("tab-events")).toHaveCount(0);
  await shot(page, "stack-rolled-back");
});

test("cloudformation__stack-create: the template, from an S3 URL, and the call that reads it", async ({ page }) => {
  await go(page, STACKS, "Stacks");
  await page.getByTestId("create-open").click();
  await expect(page.locator("h1").first()).toHaveText("Create stack");
  await expect(page.getByTestId("section-prepare")).toContainText("Not available in doze-aws: template studio");
  await page.getByTestId("template-url").fill(SEARCH_URL);
  await expect(page.getByTestId("call-panel-code").first()).toContainText("ValidateTemplate");
  await expect(page.getByTestId("call-panel-code").first()).toContainText(SEARCH_URL);
  await expect(page.getByTestId("check-note")).toContainText("The file is read from the reports bucket");
  await expect(page.getByTestId("call-panel-tab-cli")).toBeVisible();
  await shot(page, "stack-create");
});

test("cloudformation__stack-create-invalid: a file that is not a template is refused with the service's words", async ({ page, refusals }) => {
  // A file that is not a template is refused by ValidateTemplate, in the service's words.
  refusals.allow(/400 POST .*\/cloudformation\/stacks\/op\/ValidateTemplate$/);
  await go(page, `${STACKS}?do=create`, "Create stack");
  await page.getByTestId("source-file").click();
  const file = join(tmpdir(), `larder-bad-${browserKind}.yaml`);
  writeFileSync(file, "AWSTemplateFormatVersion: '2010-09-09'\nResources:\n  Queue:\n    Type: [AWS::SQS::Queue\n    Properties: { QueueName: x\n");
  await page.getByTestId("template-file").setInputFiles(file);
  await expect(page.getByTestId("file-name")).toContainText("larder-bad");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("wizard-refused")).toContainText("Template validation error.");
  await expect(page.getByTestId("wizard-refused")).toContainText("ValidationError");
  await expect(page.getByTestId("call-panel")).toContainText("The call that was refused");
  await expect(page.getByTestId("section-specify")).toContainText("This one is 0.1 KB, so the size is not the problem");
  await shot(page, "stack-create-invalid");
});

test("cloudformation__stack-create-details, options, review and changeset: the wizard to a stack waiting for review", async ({ page }) => {
  await go(page, `${STACKS}?do=create`, "Create stack");
  await page.getByTestId("template-url").fill(SEARCH_URL);
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Specify stack details");
  await expect(page.getByTestId("field-name")).toContainText("already exist");
  await expect(page.getByTestId("field-name")).toContainText("larder-core");
  await expect(page.getByTestId("param-TableName")).toContainText("Name of the DynamoDB table that holds the search index. Type String, default search-index.");
  await expect(page.getByTestId("param-input-TableName")).toHaveValue("search-index");
  await expect(page.getByTestId("param-BillingMode")).toContainText("How the table is billed");
  await expect(page.getByTestId("summary-panel")).toContainText("AWS::DynamoDB::Table");
  // The name is checked: one that is taken is said on the field.
  await page.getByTestId("stack-name").fill("larder-core");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("field-name")).toContainText("A stack named larder-core already exists in this Region.");
  await page.getByTestId("stack-name").fill("larder-search");
  await shot(page, "stack-create-details");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Configure stack options");
  const tags = page.getByTestId("tags");
  await page.getByTestId("section-tags").getByRole("button", { name: "Add tag" }).click();
  await tags.getByRole("textbox").nth(0).fill("app");
  await tags.getByRole("textbox").nth(1).fill("larder");
  await page.getByTestId("section-tags").getByRole("button", { name: "Add tag" }).click();
  await tags.getByRole("textbox").nth(2).fill("stage");
  await tags.getByRole("textbox").nth(3).fill("prod");
  await page.getByRole("combobox", { name: /Choose a topic|topic/i }).first().click();
  await page.getByRole("option", { name: "alerts" }).click();
  await expect(page.getByTestId("section-permissions")).toContainText("Stored, not applied locally.");
  await expect(page.getByTestId("tags-note")).toContainText("Applied locally.");
  await expect(page.getByTestId("notify-note")).toContainText("Applied locally.");
  await expect(page.getByTestId("rollback-note")).toBeHidden();
  await expect(page.getByTestId("section-rollback-summary")).toContainText("Not kept in doze-aws");
  await expect(page.getByTestId("policy-note")).toContainText("Applied locally.");
  await expect(page.getByTestId("section-creation")).toContainText("TimeoutInMinutes is kept and returned");
  await expect(page.getByTestId("call-panel-code").first()).toContainText('"DisableRollback": false');
  await expect(page.getByTestId("call-panel-code").first()).toContainText('"Key": "app"');
  await shot(page, "stack-create-options");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Review and create");
  await expect(page.getByTestId("stack-wizard")).toContainText("Stack name");
  await expect(page.getByTestId("stack-wizard")).toContainText("larder-search");
  await expect(page.getByTestId("stack-wizard")).toContainText("Roll back all stack resources");
  await expect(page.getByTestId("call-panel-code").first()).toContainText("CreateStack");
  await expect(page.getByTestId("call-panel-code").first()).toContainText('"StackName": "larder-search"');
  await expect(page.getByTestId("call-panel-code").first()).toContainText('"ParameterKey": "TableName"');
  await expect(page.getByTestId("review-edit-Specify stack details")).toBeVisible();
  await shot(page, "stack-create-review");
  // Edit goes back to the step, and Next comes forward to where it was.
  await page.getByTestId("review-edit-Specify stack details").click();
  await expect(page.locator("h1").first()).toHaveText("Specify stack details");
  await page.getByTestId("form-submit").click();
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Review and create");
  // Create change set: asks a name, over the review.
  await page.getByTestId("changeset-open").click();
  await expect(page.getByTestId("changeset-name-dialog")).toContainText("Create change set");
  await expect(page.getByTestId("changeset-call")).toContainText('"ChangeSetType": "CREATE"');
  await expect(page.getByTestId("changeset-call")).toContainText('"ChangeSetName": "initial-review"');
  await expect(page.getByTestId("changeset-name-dialog")).toContainText("REVIEW_IN_PROGRESS");
  await shot(page, "stack-create-changeset");
  await page.getByTestId("changeset-make").click();
  await expect(page.locator("h1").first()).toHaveText("larder-search");
  await expect(page.getByTestId("detail-badge")).toHaveText("REVIEW_IN_PROGRESS");
  await expect(page.getByTestId("stack-sentence")).toContainText("Its change set initial-review would add 2 resources: SearchIndexTable (DynamoDB table) and SearchReindexQueue (SQS queue).");
  await expect(page.getByTestId("changeset-parameters")).toContainText("search-index");
});

test("cloudformation__stack-rolled-back again: executing the change set fails on a name that is taken", async ({ page }) => {
  await go(page, stack("larder-search"), "larder-search");
  await page.getByTestId("verb-execute").click();
  await expect(page.getByTestId("executed-banner")).toContainText("Change set executed.");
  await expect(async () => {
    await page.goto(`${server.console}${stack("larder-search")}`);
    await expect(page.getByTestId("detail-badge")).toHaveText("ROLLBACK_COMPLETE", { timeout: 1000 });
  }).toPass({ timeout: 20_000 });
  await expect(page.getByTestId("stack-sentence")).toContainText("SearchIndexTable could not be made");
  await expect(page.getByTestId("stack-sentence")).toContainText("already exists");
  expect(JSON.parse(cli(["cloudformation", "describe-stacks", "--stack-name", "larder-search"])).Stacks[0].Tags.length).toBe(2);
  await shot(page, "stack-rolled-back-search");
});

test("cloudformation__stack-update, details, options, review and updated: the wizard on larder-core", async ({ page }) => {
  await go(page, stack("larder-core"), "larder-core");
  await page.getByTestId("verb-update").click();
  await expect(page.locator("h1").first()).toHaveText("Update stack");
  await expect(page.getByTestId("section-prepare")).toContainText("Use existing template");
  await expect(page.getByTestId("keep-note")).toBeVisible();
  await page.getByTestId("prepare-replace").click();
  await page.getByTestId("template-url").fill(CORE_V2_URL);
  await expect(page.getByTestId("call-panel-code").first()).toContainText("ValidateTemplate");
  await shot(page, "stack-update");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Specify stack details");
  await expect(page.getByTestId("param-input-Stage")).toHaveValue("prod");
  await expect(page.getByTestId("param-input-VisibilityTimeout")).toHaveValue("30");
  await expect(page.getByTestId("param-input-ExecutionRoleArn")).toHaveValue(/role\/lambda-exec$/);
  await expect(page.getByTestId("keep-WebhookSecret")).toBeChecked();
  await expect(page.getByTestId("param-WebhookSecret")).toContainText("Type String.");
  await expect(page.getByTestId("call-panel-code").first()).toContainText('"UsePreviousValue": true');
  await shot(page, "stack-update-details");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Configure stack options");
  await expect(page.getByTestId("tags")).toBeVisible();
  await expect(page.getByTestId("tags").locator("input").first()).toHaveValue("app");
  await expect(page.getByTestId("section-capabilities")).toContainText("Nothing to acknowledge.");
  await expect(page.getByTestId("section-creation")).toHaveCount(0);
  await expect(page.getByTestId("section-failure")).toContainText("Behavior on provisioning failure");
  await shot(page, "stack-update-options");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("Review larder-core");
  const preview = page.getByTestId("preview");
  await expect(preview.locator("tbody tr")).toHaveCount(2, { timeout: 15_000 });
  await expect(preview.locator("tr", { hasText: "ReportsBucket" })).toContainText("Add");
  await expect(preview.locator("tr", { hasText: "PickListsQueue" })).toContainText("Modify");
  await expect(page.getByTestId("view-changeset")).toBeEnabled();
  await expect(page.getByTestId("call-panel-code").first()).toContainText("UpdateStack");
  await shot(page, "stack-update-review");
  await page.getByTestId("form-submit").click();
  await expect(page.locator("h1").first()).toHaveText("larder-core");
  await expect(async () => {
    await page.goto(`${server.console}${stack("larder-core", "?tab=events&done=updated")}`);
    await expect(page.getByTestId("updated-state")).toBeVisible({ timeout: 1000 });
  }).toPass({ timeout: 20_000 });
  await expect(page.getByTestId("detail-badge")).toHaveText("UPDATE_COMPLETE");
  await expect(page.getByTestId("stack-sentence")).toContainText("larder-core was updated: ReportsBucket was created and PickListsQueue was changed.");
  await expect(page.getByTestId("stack-events")).toContainText("UPDATE_COMPLETE");
  await expect(page.getByTestId("stack-events")).toContainText("CREATE_COMPLETE");
  expect(cli(["s3api", "head-bucket", "--bucket", "larder-reports-out"])).toBeDefined();
  await shot(page, "stack-updated");
  // The preview change set the review made is gone with the page.
  const sets = JSON.parse(cli(["cloudformation", "list-change-sets", "--stack-name", "larder-core"])) as { Summaries: { ChangeSetName: string }[] };
  expect(sets.Summaries.some((s) => s.ChangeSetName.startsWith("console-preview-"))).toBe(false);
});

test("cloudformation__stack-delete and stack-deleted: what goes, what stays, and the record by id", async ({ page, refusals }) => {
  // A name that was never a stack is asked for, and refused. (The stack that is deleted is not: it is read by its ID to the end.)
  refusals.allow(/400 GET .*\/cloudformation\/stacks\/never-was$/);
  await go(page, stack("larder-core"), "larder-core");
  await page.getByTestId("verb-delete").click();
  const d = page.getByTestId("delete-stack");
  await expect(d).toContainText("Delete stack?");
  await expect(page.getByTestId("delete-owns")).toContainText("The stack owns 7 resources.");
  await expect(page.getByTestId("delete-owns")).toContainText("OrdersCreatedTopic");
  await expect(page.getByTestId("delete-kept")).toContainText("ProductImagesBucket is kept.");
  await expect(page.getByTestId("delete-kept")).toContainText("product-images stays in S3 with its objects");
  await expect(page.getByTestId("delete-allowed")).toContainText("Termination protection is off, which is why this is allowed");
  await expect(page.getByTestId("delete-allowed")).toContainText("stack ID as DELETE_COMPLETE; by name it disappears");
  await shot(page, "stack-delete");
  await page.getByTestId("delete-stack-submit").click();
  await expect(page.getByTestId("stack-deleted")).toContainText("A deleted stack is hidden from the list by default.", { timeout: 20_000 });
  await expectNoErrorAnswers(page, refusals);
  await expect(page.locator("h1").first()).toHaveText("larder-core");
  await expect(page.getByTestId("detail-badge")).toHaveText("DELETE_COMPLETE");
  await expect(page.getByTestId("stack-deleted-sentence")).toContainText("larder-core is gone");
  await expect(page.getByTestId("stack-deleted-sentence")).toContainText("ProductImagesBucket was kept, as its template asked.");
  await expect(page.getByTestId("gone-resources")).toContainText("no longer managed by CloudFormation");
  await expect(page.getByTestId("gone-events")).toContainText("DELETE_COMPLETE");
  await expect(page.getByTestId("back-to-stacks")).toBeVisible();
  await shot(page, "stack-deleted");
  // The retained bucket is still there; the deleted stack is in the list under Deleted.
  cli(["s3api", "head-bucket", "--bucket", "product-images"]);
  await page.getByTestId("back-to-stacks").click();
  await page.getByTestId("filter-status").click();
  await page.getByTestId("filter-DELETED").click();
  await expect(page.getByRole("row").filter({ hasText: "larder-core" })).toContainText("DELETE_COMPLETE");
  // A name that was never a stack says so.
  await go(page, stack("never-was"), "Stack not found");
  await expect(page.getByTestId("stack-none")).toContainText("There is no stack named never-was");
});

test("cloudformation__stacks-none and exports-none: every stack deleted", async ({ page }) => {
  cli(["cloudformation", "delete-stack", "--stack-name", "larder-edge"]);
  await expect(async () => {
    const r = aws(server.endpoint, ["cloudformation", "list-stacks", "--stack-status-filter", "DELETE_IN_PROGRESS"]);
    expect(JSON.parse(r.out).StackSummaries.length).toBe(0);
  }).toPass({ timeout: 20_000 });
  for (const name of ["larder-observability", "larder-sandbox", "larder-hotfix", "larder-nightly", "larder-search", "larder-reports-job"]) {
    cli(["cloudformation", "delete-stack", "--stack-name", name]);
  }
  await expect(async () => {
    const r = JSON.parse(cli(["cloudformation", "describe-stacks"])) as { Stacks: unknown[] };
    expect(r.Stacks.length).toBe(0);
  }).toPass({ timeout: 30_000 });
  await go(page, "/resources/cloudformation/exports", "Exports");
  await expect(page.getByTestId("list-empty")).toContainText("No stack in this region exports an output.");
  await shot(page, "exports-none");
  await go(page, STACKS, "Stacks");
  await expect(page.getByTestId("list-empty")).toContainText("You have no stacks in this region.");
  await expect(page.getByTestId("create-open")).toHaveCount(2 - 1);
  await shot(page, "stacks-none");
  void settled;
});
