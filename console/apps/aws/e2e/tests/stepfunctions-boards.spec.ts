// The 32 Step Functions boards, each reached against a real doze-aws (one per browser, with its own data, seeded as the boards' "larder"
// world with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR for a person to set beside the board.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { start, type Running, aws } from "../server";
import { expect, expectNoErrorAnswers, test } from "./fixtures";
import { type Larder, seedLarder } from "./stepfunctions-seed";
import { expectCrumbsEnd } from "./crumbs-kit";

const SHOTS = process.env.SHOTS_DIR ?? join(import.meta.dirname, "../../../../../shots-stepfunctions");
const BASE = Number(process.env.E2E_BASE_PORT ?? 45800);
const at = (b: string) => BASE + ({ firefox: 10, webkit: 20, chromium: 30 }[b] ?? 40);

test.describe.configure({ mode: "serial" });

let server: Running;
let larder: Larder;
let browserKind = "";
const cli = (args: string[]) => {
  const r = aws(server.endpoint, args);
  expect(r.code, `${args.join(" ")}: ${r.out}`).toBe(0);
  return r.out;
};

/** Photographs the page as the board is: light, then dark (the class the theme switch sets), in Chromium only. */
async function shot(page: Page, board: string) {
  if (browserKind !== "chromium") return;
  mkdirSync(SHOTS, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
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

const go = async (page: Page, path: string, title: string | RegExp) => {
  await page.goto(`${server.console}${path}`);
  await expect(page.locator("h1").first()).toHaveText(title);
};
const machine = (name: string, search = "") => `/resources/stepfunctions/machines/${name}${search}`;
const execution = (name: string) => `/resources/stepfunctions/executions/${encodeURIComponent(`arn:aws:states:us-east-1:${server.account}:execution:order-fulfilment:${name}`)}`;
const rows = (page: Page, label: string) => page.getByRole("table", { name: label }).locator("tbody tr[data-row]");
const code = (page: Page) => page.getByTestId("call-panel-code");
const choose = async (page: Page, combobox: string, option: string | RegExp) => {
  await page.getByRole("combobox", { name: combobox }).click();
  await page.getByRole("option", { name: option }).click();
};

test.beforeAll(async ({ browserName }) => {
  browserKind = browserName;
  server = await start(at(browserName), at(browserName) + 1);
  larder = seedLarder(server.endpoint, server.account);
});
test.afterAll(async () => {
  await server.stop();
});

test("stepfunctions__machines-list and machine-create-name: the machines, what runs, and the name that opens the studio", async ({ page }) => {
  await go(page, "/resources/stepfunctions/machines", "State machines");
  await expect(page.getByTestId("list-sentence")).toContainText("order-fulfilment has 2 executions running. thumbnail-batch is Express, so it keeps no execution history: its runs are written to a log group only while its logging is on.");
  const table = rows(page, "State machines");
  await expect(table).toHaveCount(2);
  await expect(table.filter({ hasText: "order-fulfilment" })).toContainText("Standard");
  await expect(table.filter({ hasText: "order-fulfilment" })).toContainText(`arn:aws:states:us-east-1:${server.account}:stateMachine:order-fulfilment`);
  await expect(table.filter({ hasText: "thumbnail-batch" })).toContainText("Express");
  await expect(page.getByTestId("activities-open")).toBeVisible();
  await expect(page.getByTestId("create-open")).toHaveText("Create state machine");
  await shot(page, "stepfunctions__machines-list");
  await page.getByTestId("type-filter").click();
  await page.getByRole("option", { name: "Express" }).click();
  await expect(table).toHaveCount(1);
  await page.getByTestId("type-filter").click();
  await page.getByRole("option", { name: "Any type" }).click();
  await expect(table).toHaveCount(2);
  await page.getByTestId("create-open").click();
  const dialog = page.getByTestId("name-dialog");
  await expect(dialog).toContainText("Start from a blank canvas and name it. You build the workflow in Workflow Studio, and nothing exists until you choose Create there.");
  await expect(dialog).toContainText("Create from blank");
  await page.getByTestId("name-input").fill("order-fulfilment");
  await expect(dialog).toContainText("order-fulfilment and thumbnail-batch already exist in this Region.");
  await page.getByTestId("name-continue").click();
  await expect(page.getByText("order-fulfilment already exists in this Region.").last()).toBeVisible();
  await page.getByTestId("name-input").fill("stock-reorder");
  await shot(page, "stepfunctions__machine-create-name");
  await page.getByTestId("name-continue").click();
  await expect(page.getByTestId("studio")).toBeVisible();
});

test("stepfunctions__machine-create-code, create-config, create-role and machine-created: the studio makes the role, then the machine", async ({ page }) => {
  await go(page, "/resources/stepfunctions/machines?do=create&name=stock-reorder", "stock-reorder");
  await expect(page.getByTestId("studio-draft")).toHaveText("Draft");
  // Design mode: states are dragged from the browser onto an edge of the canvas, and the inspector shows the chosen one.
  await expect(page.getByTestId("studio-sub")).toContainText("Workflow Studio, Design mode. Until now this is a draft: no resources have been created yet. Name and type are set in Config mode.");
  await expect(page.getByTestId("states-browser")).toContainText("Drag a state onto the canvas. Flow states control the order of steps.");
  await expect(page.getByTestId("browser-offers").locator("[data-testid^=offer-]")).toHaveText(["Choice", "Parallel", "Map", "Pass", "Wait", "Success", "Fail"]);
  await expect(page.getByTestId("browser-note")).toContainText("The Actions tab lists only what doze-aws can run: Lambda, SQS, SNS, EventBridge, DynamoDB, S3, Step Functions and the other services it serves.");
  await expect(page.getByTestId("browser-note")).toContainText("Not available in doze-aws: the .sync pattern for any service but Step Functions");
  // What the note says of Create is what the service does: it accepts such a definition (as AWS does), and the step fails when it runs.
  await expect(page.getByTestId("browser-note")).toContainText("Create accepts a definition that uses one, as AWS does, and the step fails when it runs: States.TaskFailed, which a Catch can handle.");
  expect(aws(server.endpoint, ["stepfunctions", "validate-state-machine-definition", "--definition", JSON.stringify({ StartAt: "Run", States: { Run: { Type: "Task", Resource: "arn:aws:states:::ecs:runTask.sync", Parameters: { Cluster: "c" }, End: true } } })]).out).toContain("\"result\": \"OK\"");
  await expect(page.getByTestId("canvas")).toContainText("The workflow graph updates the definition as you edit it.");
  const zone = (edge: string) => page.locator(`[data-testid=drop-zone][data-edge="${edge}"]`);
  await page.getByTestId("offer-choice").dragTo(zone("Hello>end"));
  await expect(page.getByTestId("inspector-name")).toHaveText("Choice");
  await page.getByTestId("inspector-rename").fill("CheckLevel");
  await page.getByTestId("inspector-rename").press("Enter");
  await expect(page.getByTestId("inspector-name")).toHaveText("CheckLevel");
  await page.getByTestId("browser-tabs").getByRole("radio", { name: "Actions" }).click();
  // Below the fold a drag cannot reach: a state chosen in the browser is put on an edge by choosing the edge's mark.
  await page.getByTestId("offer-sqs-send").click();
  await expect(page.getByTestId("armed-note")).toContainText("Choose where SQS SendMessage goes");
  await zone("CheckLevel>Pass").click({ force: true });
  await expect(page.getByTestId("inspector-name")).toHaveText("SQS SendMessage");
  await page.getByTestId("inspector-rename").fill("QueueReorder");
  await page.getByTestId("inspector-rename").press("Enter");
  await expect(page.getByTestId("inspector")).toContainText("Becomes Resource arn:aws:states:::sqs:sendMessage and its Parameters.");
  // A Task's fields fit the inspector's card: nothing runs past it and the page does not scroll sideways.
  const fits = await page.getByTestId("inspector").evaluate((el) => {
    const edge = el.getBoundingClientRect().right;
    const past = [...el.querySelectorAll("*")].filter((e) => e.getBoundingClientRect().right > edge + 1).map((e) => `${e.tagName.toLowerCase()}.${String(e.className).slice(0, 60)}`);
    return { card: el.scrollWidth <= el.clientWidth + 1, page: document.documentElement.scrollWidth <= window.innerWidth, past: past.slice(0, 6) };
  });
  expect(fits).toEqual({ card: true, page: true, past: [] });
  await expect(page.locator("[data-state=QueueReorder]")).toContainText("SQS SendMessage");
  await page.getByTestId("inspector-definition").click();
  await expect(page.getByTestId("inspector-json")).toContainText('"Resource": "arn:aws:states:::sqs:sendMessage"');
  await page.getByTestId("browser-tabs").getByRole("radio", { name: "Flow" }).click();
  await shot(page, "stepfunctions__machine-create-design");
  await page.getByTestId("mode-code").click();
  // The definition points QueueReorder at a state that is not there: the graph pauses and the modes are locked.
  const queue = `${server.endpoint}/${server.account}/inventory-queue`;
  const draft = {
    Comment: "Reorder stock when a SKU runs low",
    StartAt: "CheckLevel",
    States: {
      CheckLevel: { Type: "Choice", Choices: [{ Variable: "$.on_hand", NumericLessThanPath: "$.reorder_point", Next: "QueueReorder" }], Default: "EnoughStock" },
      QueueReorder: { Type: "Task", Resource: "arn:aws:states:::sqs:sendMessage", Parameters: { QueueUrl: queue, "MessageBody.$": "States.JsonToString($)" }, Next: "Notify" },
      EnoughStock: { Type: "Succeed" },
    },
  };
  await page.getByTestId("asl-editor").fill(JSON.stringify(draft, null, 2));
  await expect(page.getByTestId("studio-invalid")).toContainText("The definition is invalid. You can't switch modes while it is. Fix the definition to switch modes.");
  await expect(page.getByTestId("studio-sub")).toContainText("The definition below points QueueReorder at a state that doesn't exist: Next names Notify.");
  // The line that names the missing state is drawn in the link colour; the validator's own lines (and the end state that only follows from it) are not listed.
  const marked = page.getByTestId("asl-editor-colours").locator("span.block.text-link");
  await expect(marked).toHaveCount(1);
  await expect(marked).toHaveText("\"Next\": \"Notify\"");
  await expect(page.getByTestId("studio-diagnostics")).toHaveCount(0);
  await expect(page.getByTestId("studio-code")).not.toContainText("MISSING_");
  await expect(page.getByTestId("studio-graph-paused")).toContainText("Rendering of the graph is paused while the definition is invalid. It draws again once the definition is valid.");
  await page.getByTestId("mode-config").click();
  await expect(page.getByTestId("studio-code")).toBeVisible();
  await shot(page, "stepfunctions__machine-create-code");
  draft.States.QueueReorder = { ...draft.States.QueueReorder, Next: "EnoughStock" } as never;
  await page.getByTestId("asl-editor").fill(JSON.stringify(draft, null, 2));
  await expect(page.getByTestId("studio-invalid")).toHaveCount(0);
  await expect(page.getByTestId("sfn-graph")).toBeVisible();
  await page.getByTestId("mode-config").click();
  await expect(page.getByTestId("config-name")).toHaveValue("stock-reorder");
  await expect(page.getByTestId("role-create")).toBeChecked();
  await expect(page.getByTestId("role-create-note")).toContainText("Role to create: StepFunctions-stock-reorder-role");
  await page.getByTestId("role-review").click();
  await expect(page.getByTestId("role-review-body")).toContainText("states.amazonaws.com");
  await expect(page.getByTestId("role-review-body")).toContainText("sqs:SendMessage");
  await page.getByTestId("role-review").click();
  await page.getByTestId("config-tags").click();
  await page.getByTestId("config-tags-kv").getByRole("textbox").first().fill("app");
  await page.getByTestId("config-tags-kv").getByRole("textbox").nth(1).fill("fulfilment");
  await expect(code(page)).toContainText('"name": "stock-reorder"');
  await expect(code(page)).toContainText('"type": "STANDARD"');
  await expect(code(page)).toContainText(`"roleArn": "arn:aws:iam::${server.account}:role/StepFunctions-stock-reorder-role"`);
  await expect(code(page)).toContainText('"level": "OFF"');
  await expect(code(page)).toContainText('"key": "app"');
  await page.getByTestId("call-panel-tab-cli").click();
  await expect(code(page)).toContainText("stepfunctions create-state-machine --name stock-reorder");
  await page.getByTestId("call-panel-tab-json").click();
  await shot(page, "stepfunctions__machine-create-config");
  await page.getByTestId("form-submit").click();
  const dialog = page.getByTestId("role-dialog");
  await expect(dialog).toContainText("Step Functions creates a new IAM role for stock-reorder and then creates the state machine.");
  await expect(page.getByTestId("role-warning")).toContainText("If you delete this role, Step Functions can't recreate it.");
  await dialog.getByTestId("role-view").click();
  await expect(page.getByTestId("role-settings")).toContainText("StepFunctions-stock-reorder-role");
  await dialog.getByTestId("role-view").click();
  await shot(page, "stepfunctions__machine-create-role");
  await dialog.getByTestId("role-confirm").click();
  await expect(page.getByTestId("machine-page")).toBeVisible();
  await expect(page.getByTestId("done-banner")).toContainText("Your state machine stock-reorder and a new IAM role have been created.");
  await expect(page.getByTestId("detail-sentence")).toContainText("stock-reorder has never run. Start an execution with a JSON input; it begins at CheckLevel.");
  await expect(page.getByTestId("field-role")).toContainText(`arn:aws:iam::${server.account}:role/StepFunctions-stock-reorder-role`);
  await expect(page.getByTestId("executions-none")).toContainText("stock-reorder has not run. Start an execution to see it here.");
  await shot(page, "stepfunctions__machine-created");
  // The role the studio made is the one the machine runs as, and trusts Step Functions.
  expect(cli(["iam", "get-role", "--role-name", "StepFunctions-stock-reorder-role"])).toContain("states.amazonaws.com");
  expect(cli(["stepfunctions", "list-tags-for-resource", "--resource-arn", `arn:aws:states:us-east-1:${server.account}:stateMachine:stock-reorder`])).toContain("fulfilment");
});

test("stepfunctions__executions-none and execution-start: a machine that never ran, and the dialog that starts it", async ({ page }) => {
  await go(page, machine("stock-reorder"), "stock-reorder");
  await expect(page.getByTestId("executions-none")).toBeVisible();
  await shot(page, "stepfunctions__executions-none");
  await go(page, machine("order-fulfilment"), "order-fulfilment");
  await page.getByTestId("verb-start").click();
  const dialog = page.getByTestId("start-dialog");
  await expect(page.getByTestId("start-words")).toContainText("Starts the current revision of order-fulfilment. To start a version or an alias instead, choose it on the Versions or Aliases tab.");
  await expect(dialog).toContainText("A name is unique per state machine and can be used again 90 days after its execution closes. Starting the same name with the same input while it runs returns that run instead of a second one.");
  await expect(dialog).toContainText("The JSON that LoadOrder receives. Send {} when there is no input. Up to 256 KiB as UTF-8.");
  // It opens with the members the first state reads (LoadOrder reads $.order_id and $.created_at), so a first Start is not a run that fails at once.
  await expect(page.getByTestId("start-input")).toHaveValue(JSON.stringify({ created_at: "", order_id: "" }, null, 2));
  await page.getByTestId("start-name").fill("ord-20261108-0421");
  await page.getByTestId("start-input").fill(JSON.stringify({ order_id: "ord-20261108-0412", created_at: "2026-11-08T08:00:00Z", email: "priya@example.com" }, null, 2));
  await shot(page, "stepfunctions__execution-start");
  await page.getByTestId("start-submit").click();
  await expect(page.getByTestId("execution")).toBeVisible();
  await expect(page.locator("h1").first()).toHaveText("ord-20261108-0421");
});

test("stepfunctions__machine-detail: the header, the sentence, who it is wired to, its details and its executions", async ({ page }) => {
  await go(page, machine("order-fulfilment"), "order-fulfilment");
  await expect(page.getByTestId("detail-badge")).toHaveText(["Standard", "Active"]);
  await expect(page.getByTestId("verb-edit")).toBeVisible();
  await expect(page.getByTestId("actions-menu")).toBeVisible();
  await expect(page.getByTestId("verb-start")).toBeVisible();
  await expect(page.getByTestId("detail-sentence")).toContainText("order-fulfilment has 3 executions running. The last to fail, ord-20261108-0417, stopped at NotifyCustomer with KeyError.");
  const strip = page.getByTestId("wired-strip");
  await expect(strip).toContainText("Wired to");
  for (const name of ["orders-table", "pick-lists", "pack-order", "send-email"]) await expect(strip).toContainText(name);
  await expect(page.getByTestId("wired-self")).toHaveText("order-fulfilment");
  const details = page.getByTestId("details");
  await expect(details).toContainText(`arn:aws:states:us-east-1:${server.account}:stateMachine:order-fulfilment`);
  await expect(details).toContainText(`arn:aws:iam::${server.account}:role/larder-admin`);
  for (const t of ["Executions", "Definition", "Versions", "Aliases", "Tags"]) await expect(page.getByRole("tab", { name: t })).toBeVisible();
  const table = rows(page, "Executions");
  await expect(table.filter({ hasText: "ord-20261108-0417" })).toContainText("Failed");
  await expect(table.filter({ hasText: "ord-20261108-0417" })).toContainText("PROD");
  await expect(table.filter({ hasText: "ord-20261108-0418" })).toContainText("Running");
  await expect(table.filter({ hasText: "ord-20261108-0416" })).toContainText("Succeeded");
  await expect(page.getByRole("columnheader", { name: "Redrive count" })).toBeVisible();
  await expect(page.getByTestId("executions-note")).toContainText("Version and Alias come from the ARN a run was started with.");
  await expect(page.getByTestId("recent-calls")).toBeVisible();
  await expectCrumbsEnd(page, ["State machines","order-fulfilment"]);
  await shot(page, "stepfunctions__machine-detail");
});

test("stepfunctions__machine-definition and machine-edit: the definition read, then edited in the studio's Code mode", async ({ page }) => {
  await go(page, machine("order-fulfilment", "?tab=definition"), "order-fulfilment");
  await expect(page.getByTestId("definition-code")).toContainText('"StartAt": "LoadOrder"');
  await expect(page.getByTestId("definition-code")).toContainText('"HeartbeatSeconds": 120');
  await expect(page.getByTestId("definition-graph")).toContainText("The first state is LoadOrder.");
  await expect(page.getByTestId("definition-graph")).toContainText("DynamoDB GetItem");
  await expect(page.getByTestId("definition-graph")).toContainText("Activity pack-order");
  await expect(page.getByTestId("definition-notes")).toContainText("A definition may be up to 1,048,576 bytes. TimeoutSeconds: 3600 ends any execution still open after an hour as Timed out. PackOrder sets HeartbeatSeconds 120 and no TimeoutSeconds of its own.");
  await shot(page, "stepfunctions__machine-definition");
  await page.getByTestId("definition-edit").click();
  await expect(page.getByTestId("studio")).toBeVisible();
  await expect(page.getByTestId("studio-sub")).toContainText("Workflow Studio, Code mode. Running executions keep the definition and role they started with.");
  await expect(page.getByTestId("asl-editor")).toHaveValue(/"HeartbeatSeconds": 120/);
  // Nothing has changed, so there is nothing to save: UpdateStateMachine would make a revision of the same definition.
  await expect(page.getByTestId("studio-save")).toBeDisabled();
  const text = await page.getByTestId("asl-editor").inputValue();
  await page.getByTestId("asl-editor").fill(text.replace('"HeartbeatSeconds": 120', '"HeartbeatSeconds": 60'));
  await expect(page.getByTestId("sfn-graph")).toBeVisible();
  await shot(page, "stepfunctions__machine-edit");
  await page.getByTestId("studio-save").click();
  await expect(page.getByTestId("studio-saved")).toContainText("order-fulfilment was saved.");
  await expect(page.getByTestId("studio-save")).toBeDisabled();
  expect(cli(["stepfunctions", "describe-state-machine", "--state-machine-arn", `arn:aws:states:us-east-1:${server.account}:stateMachine:order-fulfilment`])).toContain('HeartbeatSeconds\\": 60');
  // Publish version snapshots the saved revision as the next version.
  await page.getByTestId("studio-publish").click();
  await expect(page.getByTestId("publish-words")).toContainText("as version 4, an immutable snapshot you can start by its version ARN or through an alias.");
  await expect(page.getByTestId("publish-dialog")).toContainText("Up to 256 characters. Version numbers go up by one and are never reused; a state machine holds up to 1,000.");
  await page.getByTestId("publish-description").fill("Heartbeat back to 60");
  await shot(page, "stepfunctions__version-publish");
  await page.getByTestId("publish-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("A version of order-fulfilment was published.");
  await expect(rows(page, "Versions")).toHaveCount(4);
});

test("stepfunctions__machine-versions and machine-versions-none: the snapshots, and a machine with none", async ({ page }) => {
  await go(page, machine("order-fulfilment", "?tab=versions"), "order-fulfilment");
  const table = rows(page, "Versions");
  await expect(table).toHaveCount(4);
  await expect(table.nth(0)).toContainText("Heartbeat back to 60");
  await expect(table.filter({ hasText: "Adds the pack-order activity" })).toContainText("2");
  await expect(table.filter({ hasText: "First release" })).toContainText("1");
  await expect(page.getByTestId("versions-note")).toContainText("A version is a numbered, immutable snapshot of a revision. Numbers start at 1 and are never reused");
  await page.getByTestId("choose-3").click();
  await shot(page, "stepfunctions__machine-versions");
  await page.getByTestId("version-start").click();
  await expect(page.getByTestId("start-words")).toContainText("Starts version 3 of order-fulfilment by its version ARN.");
  await page.getByTestId("start-submit").click();
  await expect(page.getByTestId("execution")).toBeVisible();
  await expect(page.getByTestId("execution-sentence")).toContainText("version 3");
  await go(page, machine("thumbnail-batch", "?tab=versions"), "thumbnail-batch");
  await expect(page.getByTestId("versions-none")).toContainText("thumbnail-batch has no published versions. A version is an immutable snapshot of its definition.");
  await shot(page, "stepfunctions__machine-versions-none");
});

test("stepfunctions__machine-aliases, alias-create and machine-aliases-none: the pointers, a split, and a machine with none", async ({ page }) => {
  await go(page, machine("order-fulfilment", "?tab=aliases"), "order-fulfilment");
  const table = rows(page, "Aliases");
  await expect(table).toHaveCount(2);
  await expect(table.filter({ hasText: "PROD" })).toContainText("Version 2, 100%");
  await expect(table.filter({ hasText: "STAGING" })).toContainText("Version 3, 100%");
  await expect(page.getByTestId("aliases-note")).toContainText("An alias points at one version, or splits executions between two by percentage.");
  await shot(page, "stepfunctions__machine-aliases");
  await page.getByTestId("alias-create").click();
  await expect(page.locator("h1").first()).toHaveText("Create alias");
  await page.getByTestId("alias-name").fill("PROD");
  await page.getByTestId("alias-create-page").getByRole("button", { name: "Create alias" }).click();
  await expect(page.getByText("PROD already exists.")).toBeVisible();
  await page.getByTestId("alias-name").fill("CANARY");
  await page.getByTestId("alias-description").fill("10 percent of runs on version 3");
  await choose(page, "Version", "Version 2");
  await expect(page.getByText("With one version, all 100 percent of the executions started from the alias run it.")).toBeVisible();
  await page.getByTestId("alias-split").click();
  await expect(page.getByTestId("alias-weight-1")).toHaveValue("90");
  await expect(page.getByTestId("alias-weight-2")).toHaveValue("10");
  await choose(page, "Second version", "Version 3");
  await expect(code(page)).toContainText('"name": "CANARY"');
  await expect(code(page)).toContainText("stateMachine:order-fulfilment:2");
  await expect(code(page)).toContainText('"weight": 90');
  await expect(page.getByText("Each percentage is 0 to 100 and the two must add up to 100: 90 + 10 = 100.")).toBeVisible();
  await expectCrumbsEnd(page, ["order-fulfilment","Aliases","Create alias"]);
  await shot(page, "stepfunctions__alias-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("done-banner")).toContainText("Alias created.");
  await expect(rows(page, "Aliases").filter({ hasText: "CANARY" })).toContainText("Version 2, 90% · Version 3, 10%");
  await go(page, machine("thumbnail-batch", "?tab=aliases"), "thumbnail-batch");
  await expect(page.getByTestId("aliases-none")).toContainText("thumbnail-batch has no aliases. An alias points to up to two versions.");
  await shot(page, "stepfunctions__machine-aliases-none");
});

test("stepfunctions__machine-tags, tags-none and tags-manage: the tags, a machine with none, and the page that changes them", async ({ page }) => {
  await go(page, machine("order-fulfilment", "?tab=tags"), "order-fulfilment");
  const table = rows(page, "Tags");
  await expect(table).toHaveCount(3);
  await expect(table.filter({ hasText: "stack" })).toContainText("larder");
  await expect(page.getByTestId("tags-note")).toContainText("Up to 50 tags per resource. Keys are 1 to 128 characters, values 0 to 256");
  await shot(page, "stepfunctions__machine-tags");
  await page.getByTestId("tags-manage").click();
  await expect(page.locator("h1").first()).toHaveText("Manage tags");
  await page.getByRole("button", { name: "Add tag" }).click();
  await page.getByTestId("tags-kv").getByRole("textbox").nth(6).fill("cost-center");
  await page.getByTestId("tags-kv").getByRole("textbox").nth(7).fill("fulfilment");
  await expect(code(page)).toContainText("TagResource");
  await expect(code(page)).toContainText('"key": "cost-center"');
  await expectCrumbsEnd(page, ["Tags","Manage tags"]);
  await shot(page, "stepfunctions__machine-tags-manage");
  await page.getByTestId("form-submit").click();
  await expect(rows(page, "Tags")).toHaveCount(4);
  const arn = `arn:aws:states:us-east-1:${server.account}:stateMachine:order-fulfilment`;
  expect(cli(["stepfunctions", "list-tags-for-resource", "--resource-arn", arn])).toContain("cost-center");
  // The aws: prefix is AWS's own: the page says so as the key is typed, and the service refuses it for the CLI too.
  await page.getByTestId("tags-manage").click();
  await page.getByRole("button", { name: "Add tag" }).click();
  await page.getByTestId("tags-kv").getByRole("textbox").nth(8).fill("aws:x");
  await expect(page.getByTestId("manage-tags-page")).toContainText("The aws: prefix is reserved.");
  const refusedTag = aws(server.endpoint, ["stepfunctions", "tag-resource", "--resource-arn", arn, "--tags", "key=aws:x,value=1"]);
  expect(refusedTag.code).not.toBe(0);
  expect(refusedTag.out).toContain("ValidationException");
  await page.getByTestId("form-cancel").click();
  await go(page, machine("thumbnail-batch", "?tab=tags"), "thumbnail-batch");
  await expect(page.getByTestId("tags-none")).toContainText("thumbnail-batch has no tags.");
  await shot(page, "stepfunctions__machine-tags-none");
});

test("stepfunctions__execution-running and execution-stop: a run waiting for a worker, and the dialog that stops it", async ({ page }) => {
  await go(page, execution("ord-20261108-0418"), "ord-20261108-0418");
  await expect(page.getByTestId("execution-status")).toHaveText("Running");
  await expect(page.getByTestId("execution-sentence")).toContainText("on order-fulfilment, version 3. It is waiting for a worker on PackOrder.");
  await expect(page.getByTestId("execution-arn")).toHaveText(`arn:aws:states:us-east-1:${server.account}:execution:order-fulfilment:ord-20261108-0418`);
  await expect(page.getByTestId("graph-card")).toContainText("The first two steps succeeded. PackOrder is in progress: no worker has taken the task yet.");
  await expect(page.locator("[data-state=PackOrder]")).toHaveAttribute("data-status", "running");
  await expect(page.getByTestId("step-name")).toHaveText("PackOrder");
  await expect(page.getByTestId("step-note")).toContainText("PackOrder is an activity task. It stays in progress until a worker calls GetActivityTask for pack-order, then answers with SendTaskSuccess or SendTaskFailure. With HeartbeatSeconds 120 the worker must call SendTaskHeartbeat within that time.");
  await shot(page, "stepfunctions__execution-running");
  await page.getByTestId("stop-open").click();
  const dialog = page.getByTestId("stop-dialog");
  await expect(dialog).toContainText("Stop ord-20261108-0418 while it runs? The execution ends with the status Aborted.");
  await expect(page.getByTestId("stop-left")).toContainText("LoadOrder (DynamoDB GetItem on orders-table), CreatePickList (SQS SendMessage on pick-lists) already ran. Stopping does not undo what they did.");
  await expect(dialog).toContainText("Only Standard executions can be stopped, and only while they run. An aborted execution can be redriven for 14 days after it ends. Stop does not undo what finished steps already did.");
  await shot(page, "stepfunctions__execution-stop");
  await page.getByTestId("stop-submit").click();
  await expect(page.getByTestId("execution-status")).toHaveText("Aborted");
});

test("stepfunctions__execution-failed and execution-redrive: why it failed, the way to recover, and what redrive does", async ({ page }) => {
  await go(page, execution("ord-20261108-0417"), "ord-20261108-0417");
  await expect(page.getByTestId("execution-status")).toHaveText("Failed");
  await expect(page.getByTestId("execution-sentence")).toContainText("on order-fulfilment, PROD, version 2.");
  await expect(page.getByTestId("execution-sentence")).toContainText("and stopped at NotifyCustomer.");
  await expect(page.getByTestId("execution-failure")).toContainText("The execution failed at NotifyCustomer.");
  await expect(page.getByTestId("failure-error")).toHaveText("KeyError: 'email'");
  await expect(page.getByTestId("graph-card")).toContainText("Three steps succeeded and the fourth failed. Choose a step to see its input, output and error.");
  await expect(page.getByTestId("step-name")).toHaveText("NotifyCustomer");
  await expect(page.getByTestId("step-info")).toContainText("arn:aws:states:::lambda:invoke");
  await expect(page.getByTestId("step-info")).toContainText("Function send-email");
  await expect(page.getByTestId("step-info")).toContainText("KeyError");
  await expect(page.getByTestId("step-note")).toContainText("The Retry rule on NotifyCustomer matches Lambda.ServiceException only, so this error was not retried.");
  await expect(page.getByTestId("step-note")).toContainText("until send-email or the input changes");
  // The cause is told further when the input shows why, and the graph says what each step took.
  await expect(page.getByTestId("cause-hint")).toHaveText("send-email read event['email'] and the input has none");
  await expect(page.getByTestId("state-took")).toHaveCount(4);
  await expect(page.locator("[data-state=NotifyCustomer]").getByTestId("state-took")).toContainText(/\d/);
  await page.getByTestId("step-views").getByRole("radio", { name: "Retry" }).click();
  await expect(page.getByTestId("step-retry")).toContainText("Lambda.ServiceException");
  await page.getByTestId("step-views").getByRole("radio", { name: "Details" }).click();
  await page.getByTestId("failure-cause").click();
  await expect(page.getByTestId("failure-cause-text")).toContainText("errorMessage");
  await expectCrumbsEnd(page, ["Executions", ""]);
  await page.getByTestId("recover-menu").click();
  await expect(page.getByTestId("recover-redrive")).toHaveText("Redrive from failure");
  await shot(page, "stepfunctions__execution-failed");
  await page.getByTestId("recover-redrive").click();
  const dialog = page.getByTestId("redrive-dialog");
  await expect(page.getByTestId("redrive-words")).toContainText("Redrive continues ord-20261108-0417 from the step that failed, NotifyCustomer, with the same input. The 3 steps that succeeded are not run again and their results stay in the history.");
  await expect(page.getByTestId("redrive-warning")).toContainText("The input has no email. send-email reads event['email'], so NotifyCustomer fails again unless the function changes. A new execution with the email added would not.");
  await expect(dialog).toContainText("Same definition (PROD, version 2) and same execution ARN. The state machine timeout and the retry count of the rerun step start over. Possible for 14 days after the execution ended. The execution summary then shows Redrive count 1.");
  await shot(page, "stepfunctions__execution-redrive");
  await page.getByTestId("redrive-submit").click();
  await expect(dialog).toBeHidden();
  // The same step fails again, and the summary counts the redrive.
  await expect(async () => {
    await page.getByRole("button", { name: "Refresh" }).first().click();
    await page.getByTestId("execution-status").waitFor();
    expect(JSON.parse(cli(["stepfunctions", "describe-execution", "--execution-arn", `arn:aws:states:us-east-1:${server.account}:execution:order-fulfilment:ord-20261108-0417`])).redriveCount).toBe(1);
  }).toPass({ timeout: 15_000 });
});

test("stepfunctions__execution-table and execution-events: each step as a row, and every event", async ({ page }) => {
  await go(page, execution("ord-20261108-0416"), "ord-20261108-0416");
  await page.getByTestId("tab-table").click();
  const table = rows(page, "Steps");
  await expect(table).toHaveCount(4);
  await expect(table.nth(0)).toContainText("LoadOrder");
  await expect(table.nth(0)).toContainText("DynamoDB GetItem");
  await expect(table.nth(2)).toContainText("Activity pack-order");
  await expect(table.nth(3)).toContainText("Lambda Invoke");
  await expect(page.getByRole("columnheader", { name: "Started After" })).toBeVisible();
  await expect(page.getByRole("columnheader", { name: "Duration" })).toBeVisible();
  // The table and the events are the views of the steps alone: no failure banner and no summary above them. Preferences picks the columns.
  await expect(page.getByTestId("execution-details")).toHaveCount(0);
  await expect(page.getByTestId("execution-failure")).toHaveCount(0);
  await page.getByTestId("steps-preferences").click();
  await page.getByTestId("steps-column-d").click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("columnheader", { name: "Duration" })).toHaveCount(0);
  await expect(page.getByRole("columnheader", { name: "Status" })).toBeVisible();
  await page.getByTestId("steps-preferences").click();
  await page.getByTestId("steps-column-d").click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("columnheader", { name: "Duration" })).toBeVisible();
  await page.getByTestId("steps-filter").fill("lambda");
  await expect(table).toHaveCount(1);
  await page.getByTestId("steps-filter").fill("");
  await expect(table).toHaveCount(4);
  await table.nth(3).click();
  await expect(page.getByTestId("step-name")).toHaveText("NotifyCustomer");
  await shot(page, "stepfunctions__execution-table");
  await page.getByTestId("tab-events").click();
  await expect(page.getByTestId("execution-details")).toHaveCount(0);
  await expect(page.getByTestId("history-event").first()).toContainText("ExecutionStarted");
  await expect(page.getByTestId("history-line")).toContainText("events, 1 to");
  await expect(page.getByTestId("history-note")).toContainText("A page shows up to 25 events, and an execution's history is limited to 25,000 events.");
  await page.getByTestId("events-filter").fill("ActivityScheduled");
  await expect(page.getByTestId("history-event")).toHaveCount(1);
  await page.getByTestId("events-filter").fill("");
  const before = await page.getByTestId("history-event").first().innerText();
  await page.getByTestId("history-sort").click();
  expect(await page.getByTestId("history-event").first().innerText()).not.toBe(before);
  await page.getByTestId("history-sort").click();
  await page.getByRole("button", { name: "Open event 2", exact: true }).click();
  await expect(page.getByTestId("execution-events")).toContainText("LoadOrder");
  // A run that succeeded opens nothing; one that failed opens on the event that failed, and says which.
  await expect(page.getByTestId("history-expanded")).toHaveCount(1);
  await page.getByRole("button", { name: "Close event 2", exact: true }).click();
  await expect(page.getByTestId("history-expanded")).toHaveCount(0);
  await shot(page, "stepfunctions__execution-events");
  await go(page, execution("ord-20261108-0417"), "ord-20261108-0417");
  await page.getByTestId("tab-events").click();
  await expect(page.getByTestId("history-expanded")).toHaveText(/^The expanded event is event \d+, \w*Failed on NotifyCustomer\.$/);
  await expect(page.locator("[data-testid=history-event][data-event]").filter({ hasText: "ExecutionFailed" })).toHaveClass(/bg-bad/);
});

test("stepfunctions__activities-list and activity-create: the workers' tasks, and a new one", async ({ page }) => {
  await go(page, "/resources/stepfunctions/machines", "State machines");
  // Minutes of browsing read the record many times over: the console's own reads must not push the worker's calls out of the window
  // this page reads (it asked for the newest 200, and the console's reads alone fill that).
  await page.evaluate(async () => {
    for (let i = 0; i < 300; i++) await fetch("/api/v1/resources/stepfunctions/activities", { headers: { "X-Console": "1" } });
  });
  await page.getByTestId("activities-open").click();
  await expect(page.locator("h1").first()).toHaveText("Activities");
  await expect(rows(page, "Activities")).toHaveCount(2);
  await expect(rows(page, "Activities").filter({ hasText: "pack-order" })).toContainText(`arn:aws:states:us-east-1:${server.account}:activity:pack-order`);
  await expect(page.getByTestId("list-sentence")).toContainText("polls pack-order, which order-fulfilment uses in its PackOrder step. print-label is not used by any machine yet.");
  await expect(page.getByTestId("activities-note")).toContainText("A worker asks for a task with GetActivityTask, which holds the request for up to 60 seconds");
  await expect(page.getByTestId("worker-calls")).toContainText("GetActivityTask");
  await shot(page, "stepfunctions__activities-list");
  await page.getByTestId("create-open").click();
  await expect(page.locator("h1").first()).toHaveText("Create activity");
  await expect(page.getByTestId("activity-create-page")).toContainText("pack-order and print-label already exist; creating a name that exists returns the existing activity and leaves its tags alone.");
  await page.getByTestId("activity-name-input").fill("inspect-order");
  await expect(code(page)).toContainText('"name": "inspect-order"');
  // The service keeps an encryption setting on an activity and does nothing with it: a switch, said so, and the call carries it.
  await expect(page.getByTestId("activity-encryption")).toContainText("Stored, not applied locally.");
  await page.getByTestId("activity-kms").click();
  await expect(page.getByTestId("activity-kms-period")).toHaveValue("300");
  await page.getByTestId("activity-kms").click();
  await expect(code(page)).not.toContainText("encryptionConfiguration");
  await expectCrumbsEnd(page, ["Activities","Create activity"]);
  await shot(page, "stepfunctions__activity-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("Activity inspect-order created.");
  await expect(rows(page, "Activities")).toHaveCount(3);
});

test("stepfunctions__machine-delete, machines-none and activities-none: the delete dialog, and what is left when everything is gone", async ({ page, refusals }) => {
  await go(page, machine("order-fulfilment"), "order-fulfilment");
  // The seeded runs wait on an activity with a 120 second heartbeat and end by themselves a couple of minutes after the seed: two fresh ones make sure some are still open.
  for (const n of ["late-1", "late-2"]) cli(["stepfunctions", "start-execution", "--state-machine-arn", `arn:aws:states:us-east-1:${server.account}:stateMachine:order-fulfilment`, "--name", n, "--input", JSON.stringify({ order_id: "ord-20261107-1893", created_at: "2026-11-08T08:00:00Z", email: "priya@example.com" })]);
  const running = (JSON.parse(cli(["stepfunctions", "list-executions", "--state-machine-arn", `arn:aws:states:us-east-1:${server.account}:stateMachine:order-fulfilment`, "--status-filter", "RUNNING"])) as { executions: unknown[] }).executions.length;
  expect(running).toBeGreaterThan(1);
  await page.getByTestId("actions-menu").click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  const dialog = page.getByTestId("delete-dialog");
  await expect(dialog).toContainText("Deleting order-fulfilment also deletes its 4 versions and 3 aliases.");
  await expect(page.getByTestId("delete-running")).toContainText(`${running} executions are running.`);
  await expect(page.getByTestId("delete-difference")).toContainText("doze-aws deletes the state machine at once and does not stop what is running");
  await shot(page, "stepfunctions__machine-delete");
  // Nothing reads what is gone again: no request that came back refused is made once the delete has answered.
  const refused: string[] = [];
  page.on("response", (r) => {
    if (r.status() >= 400) refused.push(`${r.status()} ${r.url()}`);
  });
  await page.getByTestId("delete-dialog-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("State machine order-fulfilment deleted.");
  await expectNoErrorAnswers(page, refusals);
  await expect(rows(page, "State machines")).toHaveCount(2);
  expect(refused).toEqual([]);
  expect(cli(["stepfunctions", "list-state-machines"])).not.toContain("order-fulfilment");
  for (const name of ["thumbnail-batch", "stock-reorder"]) cli(["stepfunctions", "delete-state-machine", "--state-machine-arn", `arn:aws:states:us-east-1:${server.account}:stateMachine:${name}`]);
  await go(page, "/resources/stepfunctions/machines", "State machines");
  await expect(page.getByTestId("list-empty")).toContainText("You have no state machines in this region. Create one to orchestrate steps across services.");
  await shot(page, "stepfunctions__machines-none");
  for (const name of ["pack-order", "print-label", "inspect-order"]) cli(["stepfunctions", "delete-activity", "--activity-arn", `arn:aws:states:us-east-1:${server.account}:activity:${name}`]);
  await go(page, "/resources/stepfunctions/activities", "Activities");
  await expect(page.getByTestId("list-empty")).toContainText("An activity lets a worker you run yourself take part in a state machine.");
  await shot(page, "stepfunctions__activities-none");
});

test("a state machine and an execution that are not there: the same page as every kind", async ({ page, refusals }) => {
  // A page of what is not there asks the service, which refuses.
  refusals.allow(/400 GET .*\/stepfunctions\/machines\/no-such-machine$/);
  refusals.allow(/400 POST .*\/stepfunctions\/machines\/order-fulfilment\/op\/(DescribeExecution|DescribeStateMachineForExecution|GetExecutionHistory)$/);
  await page.goto(`${server.console}${machine("no-such-machine")}`);
  await expect(page.locator("h1").first()).toHaveText("State machine not found");
  await expect(page.getByTestId("detail-error")).toContainText("There is no state machine named no-such-machine");
  await page.goto(`${server.console}${execution("no-such-run")}`);
  await expect(page.getByTestId("execution-error")).toContainText("There is no execution named no-such-run");
  expect(larder.machine).toBe("order-fulfilment");
});

test("the studio's Design mode: a pattern, the keyboard path, a state moved, undo and redo, and a Choice's rules", async ({ page }) => {
  await go(page, "/resources/stepfunctions/machines?do=create&name=design-extras", "design-extras");
  const zone = (edge: string) => page.locator(`[data-testid=drop-zone][data-edge="${edge}"]`);
  const state = (name: string) => page.locator(`[data-state="${name}"]`);
  // The keyboard path of a drag: choose the state in the browser with Enter, then reach a mark with Tab and choose it with Enter.
  await page.getByTestId("offer-wait").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("armed-note")).toContainText("Choose where Wait goes");
  await zone("start>Hello").focus();
  await page.keyboard.press("Enter");
  await expect(state("Wait")).toBeVisible();
  // Undo takes the drop back, Redo puts it back; Ctrl or Cmd with Z does the same outside a text field.
  await expect(page.getByTestId("design-redo")).toBeDisabled();
  await page.getByTestId("design-undo").click();
  await expect(state("Wait")).toHaveCount(0);
  await expect(page.getByTestId("design-undo")).toBeDisabled();
  await page.getByTestId("design-redo").click();
  await expect(state("Wait")).toBeVisible();
  await page.getByTestId("design-undo").focus();
  await page.keyboard.press("ControlOrMeta+z");
  await expect(state("Wait")).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+Shift+z");
  await expect(state("Wait")).toBeVisible();
  // A state is moved by choosing it, choosing Move state, and choosing the mark it goes to; next to itself is refused.
  await state("Wait").click();
  await page.getByTestId("inspector-move").click();
  await expect(page.getByTestId("armed-note")).toContainText("Choose where Wait moves to");
  await zone("start>Wait").click({ force: true });
  await expect(page.getByTestId("design-problem")).toContainText("Wait is already there");
  await zone("Hello>end").click({ force: true });
  await expect(page.getByTestId("armed-note")).toHaveCount(0);
  await page.getByTestId("mode-code").click();
  const moved = JSON.parse(await page.getByTestId("asl-editor").inputValue()) as { StartAt: string; States: Record<string, { Next?: string; End?: boolean }> };
  expect(moved.StartAt).toBe("Hello");
  expect(moved.States.Hello?.Next).toBe("Wait");
  expect(moved.States.Wait?.End).toBe(true);
  // The modes switch once the definition has been checked (it is, a moment after the last change).
  await expect(page.getByTestId("sfn-graph")).toBeVisible();
  await page.getByTestId("mode-design").click();
  await expect(page.getByTestId("studio-design")).toBeVisible();
  // A pattern brings the states that go together: the Lambda with Retry and Catch brings the Fail it catches to.
  await page.getByTestId("browser-tabs").getByRole("radio", { name: "Patterns" }).click();
  await page.getByTestId("offer-retry-catch").click();
  await zone("Hello>Wait").click({ force: true });
  await expect(page.getByTestId("inspector-name")).toHaveText("Lambda Invoke");
  await expect(state("Handle failure")).toBeVisible();
  // A Choice's rules are edited in its inspector, and the definition takes them.
  await page.getByTestId("browser-tabs").getByRole("radio", { name: "Flow" }).click();
  await page.getByTestId("offer-choice").click();
  await zone("Wait>end").click({ force: true });
  await expect(page.getByTestId("inspector-name")).toHaveText("Choice");
  await expect(page.getByTestId("inspector-move")).toHaveCount(0);
  await page.getByTestId("inspector-choices").fill(JSON.stringify([{ Variable: "$.count", NumericGreaterThan: 3, Next: "Pass" }]));
  await page.getByTestId("mode-code").click();
  await expect(page.getByTestId("asl-editor")).toHaveValue(/"NumericGreaterThan": 3/);
});
