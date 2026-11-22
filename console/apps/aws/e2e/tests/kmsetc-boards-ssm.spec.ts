// The 13 Parameter Store boards, each reached against a real doze-aws (one per browser, seeded as the boards' "larder" world with the
// real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR.
import { CUT, expect, test } from "./fixtures";
import { start, type Running } from "../server";
import { seedKmsetc } from "./kmsetc-seed";
import { cliOf, open, portsFor, shoot } from "./kmsetc-kit";

test.describe.configure({ mode: "serial" });

let server: Running;
let browser = "";
let cli: (a: string[]) => string;
const shot = (page: Parameters<typeof shoot>[0], board: string) => shoot(page, browser, board);
const PASSWORD = "%2Flarder%2Fprod%2Fdb%2Fpassword";
const at = (name: string, q = "") => `/resources/ssm/parameters/${name}${q}`;

test.beforeAll(async ({ browserName }) => {
  browser = browserName;
  const [l, c] = portsFor(browserName, 2);
  server = await start(l, c);
  cli = cliOf(server);
  seedKmsetc(server.endpoint, server.account);
});
test.afterAll(async () => {
  await server.stop();
});

test("ssm__parameters-list: the parameters with their tier, type, version and description, and a sentence", async ({ page }) => {
  await open(page, server.console, "/resources/ssm/parameters", "Parameter Store");
  await expect(page.getByTestId("list-sentence")).toContainText("There are 8 parameters in us-east-1: 7 Standard and 1 Advanced.");
  await expect(page.getByTestId("list-sentence")).toContainText("/larder/prod/db/password is the only SecureString.");
  await expect(page.getByTestId("list-sentence")).toContainText("The newest change is /larder/staging/feature/new-checkout, saved as version 2");
  await expect(page.getByRole("table", { name: "Parameters" }).locator("tbody tr[data-row]")).toHaveCount(8);
  await expect(page.getByRole("table", { name: "Parameters" })).toContainText("Origins the public API accepts");
  await expect(page.getByTestId("page-line")).toHaveText("1 to 8 of 8 parameters");
  await shot(page, "ssm__parameters-list");
});

test("ssm__parameters-staging: a name filter that begins with a path narrows the list on the server", async ({ page }) => {
  await open(page, server.console, "/resources/ssm/parameters", "Parameter Store");
  await page.getByTestId("filter-text").fill("/larder/staging");
  await page.getByTestId("filter-add").click();
  await expect(page.getByTestId("filter-chip")).toContainText("Name begins with: /larder/staging");
  await expect(page.getByTestId("list-sentence")).toContainText("The filter matches 2 of 8 parameters, all Standard String parameters.");
  await expect(page.getByRole("table", { name: "Parameters" }).locator("tbody tr[data-row]")).toHaveCount(2);
  await shot(page, "ssm__parameters-staging");
  await page.getByTestId("filter-remove").click();
  await page.getByTestId("filter-option").click();
  await page.getByRole("option", { name: "contains" }).click();
  await page.getByTestId("filter-text").fill("db/");
  await page.getByTestId("filter-add").click();
  await expect(page.getByRole("table", { name: "Parameters" }).locator("tbody tr[data-row]")).toHaveCount(3);
});

// The refusal of a name that exists is the service's own 400, which the browser logs as a failed load.
test.describe("a name that exists", () => {
  test.use({ allowErrors: CUT });
  test("ssm__parameter-create, create-exists, created: a form with the call beside it, a refusal on the name, then the list says what came back", async ({ page }) => {
    await open(page, server.console, "/resources/ssm/parameters?create=yes", "Create parameter");
    await page.getByTestId("parameter-name").fill("/larder/prod/db/password");
    await expect(page.getByTestId("field-name")).toContainText("already exists in us-east-1");
    await page.getByTestId("tier-Standard").click();
    await page.getByTestId("type-SecureString").click();
    await page.getByRole("combobox", { name: "Encryption key" }).click();
    await page.getByRole("option", { name: "alias/larder-app" }).click();
    await page.getByTestId("parameter-description").fill("Read replica password");
    await page.getByTestId("parameter-value").fill("r3plica-Kx82-vQ0m-ZT19");
    await page.getByTestId("parameter-tags").getByRole("button", { name: "Add tag" }).click();
    await page.getByTestId("parameter-tags").getByRole("textbox").nth(0).fill("app");
    await page.getByTestId("parameter-tags").getByRole("textbox").nth(1).fill("larder");
    await expect(page.getByTestId("call-panel")).toContainText('"KeyId": "alias/larder-app"');
    await expect(page.getByTestId("call-panel")).toContainText('"Tier": "Standard"');
    await shot(page, "ssm__parameter-create");
    await page.getByTestId("form-submit").click();
    await expect(page.getByTestId("put-refused")).toContainText("PutParameter was refused with ParameterAlreadyExists (HTTP 400).");
    await expect(page.getByTestId("field-name")).toContainText("choose Edit on /larder/prod/db/password");
    await expect(page.getByTestId("call-panel")).toContainText("The call that was refused");
    await shot(page, "ssm__parameter-create-exists");
    await page.getByTestId("parameter-name").fill("/larder/prod/db/replica-password");
    await page.getByTestId("form-submit").click();
    await expect(page.getByTestId("created-banner")).toContainText("PutParameter returned version 1 for /larder/prod/db/replica-password, a SecureString.");
    await expect(page.getByTestId("list-sentence")).toContainText("There are 9 parameters");
    await expect(page.getByRole("table", { name: "Parameters" })).toContainText("/larder/prod/db/replica-password");
    await shot(page, "ssm__parameter-created");
    expect(JSON.parse(cli(["ssm", "get-parameter", "--name", "/larder/prod/db/replica-password", "--with-decryption"])).Parameter.Value).toBe("r3plica-Kx82-vQ0m-ZT19");
  });
});

test("ssm__parameter-detail, parameter-detail-revealed: a parameter's page, its value masked until Show", async ({ page }) => {
  await open(page, server.console, at(PASSWORD), "/larder/prod/db/password");
  await expect(page.getByTestId("detail-sentence")).toContainText("is a SecureString at version 3");
  await expect(page.getByTestId("parameter-details")).toContainText("Database password for the prod stack");
  await expect(page.getByTestId("parameter-details")).toContainText("alias/larder-app");
  await expect(page.getByTestId("tab-history")).toContainText("3");
  await expect(page.getByTestId("tab-tags")).toContainText("3");
  await expect(page.getByTestId("parameter-value-card")).not.toContainText("kq7Vz");
  await shot(page, "ssm__parameter-detail");
  await page.getByTestId("value-show").click();
  await expect(page.getByTestId("parameter-value")).toHaveText("kq7Vz-2mPf-Lr9Xw-Tn40");
  await expect(page.getByTestId("detail-sentence")).toContainText("decrypted for this page view");
  await shot(page, "ssm__parameter-detail-revealed");
  await page.getByTestId("value-hide").click();
  await expect(page.getByTestId("parameter-value-card")).not.toContainText("kq7Vz");
});

test("ssm__parameter-history, parameter-labels: the versions with their labels, and Manage labels moving one", async ({ page }) => {
  await open(page, server.console, at(PASSWORD, "?tab=history"), "/larder/prod/db/password");
  await expect(page.getByRole("table", { name: "Versions" })).toContainText("canary");
  await expect(page.getByRole("table", { name: "Versions" })).toContainText("current");
  await expect(page.getByTestId("labels-open")).toBeDisabled();
  await shot(page, "ssm__parameter-history");
  await page.getByTestId("pick-3").check();
  await page.getByTestId("labels-open").click();
  await page.getByTestId("label-name").fill("current");
  await expect(page.getByTestId("label-move")).toContainText("current is on version 2 now. Saving moves it to version 3");
  await page.getByTestId("label-add").click();
  await page.getByTestId("remove-canary").click();
  await expect(page.getByTestId("labels-save")).toBeDisabled();
  await page.getByTestId("label-confirm").fill("Confirm");
  await shot(page, "ssm__parameter-labels");
  await page.getByTestId("labels-save").click();
  await expect(page.getByRole("table", { name: "Versions" }).locator("tr[data-row]", { hasText: "3" }).first()).toContainText("current");
  const hist = JSON.parse(cli(["ssm", "get-parameter-history", "--name", "/larder/prod/db/password"])).Parameters;
  expect(hist.find((p: { Version: number }) => p.Version === 3).Labels).toEqual(["current"]);
  expect(hist.find((p: { Version: number }) => p.Version === 2).Labels).toEqual([]);
});

test("ssm__parameter-tags: the tags of a parameter, edited in place", async ({ page }) => {
  await open(page, server.console, at(PASSWORD, "?tab=tags"), "/larder/prod/db/password");
  await expect(page.getByTestId("tags-count")).toHaveText("3 tags");
  await page.getByTestId("tags-edit-open").click();
  await shot(page, "ssm__parameter-tags");
  await page.getByTestId("tags-kv").getByRole("button", { name: /Add/ }).click();
  await page.getByTestId("tags-kv").getByRole("textbox").nth(6).fill("owner");
  await page.getByTestId("tags-kv").getByRole("textbox").nth(7).fill("ops");
  await page.getByTestId("tags-save").click();
  await expect(page.getByTestId("tags-saved")).toBeVisible();
  expect(JSON.parse(cli(["ssm", "list-tags-for-resource", "--resource-type", "Parameter", "--resource-id", "/larder/prod/db/password"])).TagList).toHaveLength(4);
});

test("ssm__parameter-edit: Edit saves the next version with Overwrite", async ({ page }) => {
  await open(page, server.console, at(PASSWORD), "/larder/prod/db/password");
  await page.getByTestId("verb-edit").click();
  await expect(page.locator("h1").first()).toHaveText("Edit");
  await expect(page.getByTestId("parameter-name")).toHaveAttribute("readonly", "");
  await expect(page.getByTestId("type-SecureString")).toBeDisabled();
  await page.getByTestId("parameter-value").fill("Zt3Mw-8cQd-Hn5Rb-Ve62");
  await expect(page.getByTestId("call-panel")).toContainText('"Overwrite": true');
  await shot(page, "ssm__parameter-edit");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("saved-banner")).toContainText("PutParameter returned version 4");
  expect(JSON.parse(cli(["ssm", "get-parameter", "--name", "/larder/prod/db/password", "--with-decryption"])).Parameter.Value).toBe("Zt3Mw-8cQd-Hn5Rb-Ve62");
});

test("ssm__parameters-delete, parameters-deleted: tick two, Delete parameters, and the list says what DeleteParameters returned", async ({ page }) => {
  await open(page, server.console, "/resources/ssm/parameters", "Parameter Store");
  await page.getByTestId("filter-text").fill("/larder/staging");
  await page.getByTestId("filter-add").click();
  await page.getByTestId("select-all").click();
  await page.getByTestId("delete-selected").click();
  await expect(page.getByTestId("delete-names")).toContainText("/larder/staging/db/host");
  await expect(page.getByTestId("delete-names")).toContainText("/larder/staging/feature/new-checkout");
  await shot(page, "ssm__parameters-delete");
  await page.getByTestId("delete-parameters-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("DeleteParameters returned 2 deleted names, /larder/staging/db/host and /larder/staging/feature/new-checkout, and no invalid parameters.");
  await expect(page.getByTestId("deleted-banner")).toContainText("Now 7 parameters are left in us-east-1.");
  await expect(page.getByText("0 parameters match")).toBeVisible();
  await shot(page, "ssm__parameters-deleted");
  expect(JSON.parse(cli(["ssm", "describe-parameters"])).Parameters).toHaveLength(7);
});

test.describe("a parameter that does not exist", () => {
  test.use({ allowErrors: CUT });
  test("has a page of its own that says so", async ({ page }) => {
    await page.goto(`${server.console}${at("%2Fnope")}`);
    await expect(page.getByTestId("detail-error")).toContainText("/nope");
  });
});
