// The usage plan boards of API Gateway (the list, create, a plan with its stages and API keys, and the empty states), each reached against a real doze-aws
// (one per browser, seeded as the boards' "larder" world) and, in Chromium, photographed light and dark. The REST boards are apigateway-boards.spec.ts, the
// HTTP boards apigateway-boards-http.spec.ts.
import { advance, expect, expectNoErrorAnswers, test } from "./fixtures";
import { apiPath, begin, choose, cli, ctx, end, go, json, shot } from "./apigateway-kit";

test.describe.configure({ mode: "serial" });

test.beforeAll(async ({ browserName }) => begin(browserName));
test.afterAll(end);

test("apigateway__rest-usage-plans: the plans with their throttle, quota and stages, and that doze-aws enforces them", async ({ page }) => {
  await go(page, "/resources/apigateway/usageplans", "Usage plans");
  await expect(page.getByTestId("list-sentence")).toContainText("partners-basic covers larder-public:prod. A method that requires an API key is served only with a key from a plan like this one.");
  await expect(page.getByTestId("account-nav")).toContainText("APIs");
  const table = page.getByRole("table", { name: "Usage plans" });
  await expect(table).toContainText("partners-basic");
  await expect(table).toContainText("Partner storefronts, basic tier");
  await expect(table).toContainText("50 per second, burst 100");
  await expect(table).toContainText("10,000 per month");
  await expect(table).toContainText("larder-public:prod");
  await expect(page.getByText("Enforced here.")).toBeVisible();
  await expect(page.getByText("Stored, not applied locally")).toBeHidden();
  await shot(page, "rest-usage-plans");
});

test("apigateway__rest-usage-plan-create: a name, a throttle and a quota, the call, and a landing on the new plan", async ({ page }) => {
  await go(page, "/resources/apigateway/usageplans?do=create", "Create usage plan");
  await page.getByTestId("plan-name").fill("partners-premium");
  await page.getByTestId("plan-description").fill("Partner storefronts, premium tier");
  // Throttling and Quota are toggles: off, a plan has neither and the call carries neither; on, each opens its fields.
  await expect(page.getByTestId("plan-rate")).toHaveCount(0);
  await expect(page.getByTestId("call-panel")).not.toContainText("throttle");
  await page.getByTestId("plan-throttle").click();
  await page.getByTestId("plan-quota").click();
  await page.getByTestId("plan-rate").fill("200");
  await page.getByTestId("plan-burst").fill("400");
  await page.getByTestId("plan-limit").fill("100000");
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText("CreateUsagePlan");
  await expect(call).toContainText('"rateLimit": 200');
  await expect(call).toContainText('"burstLimit": 400');
  await expect(call).toContainText('"period": "MONTH"');
  await expect(call).toContainText("No apiStages yet: the plan covers nothing until you add a stage to it.");
  await expect(page.getByTestId("section-throttle")).toContainText("partners-basic uses 50 per second, burst 100.");
  await shot(page, "rest-usage-plan-create");
  await advance(page);
  await expect(page.getByTestId("created-banner")).toContainText("Usage plan partners-premium created.");
  await expect(page.locator("h1")).toHaveText("partners-premium");
  await expect(page.getByTestId("plan-details")).toContainText("200 requests per second, burst 400");
  await expect(page.getByTestId("plan-details")).toContainText("100,000 requests per month");
});

test("apigateway__rest-usage-plan: the plan's stages and keys, and a key added to it", async ({ page, refusals }) => {
  await go(page, apiPath("usageplans", ctx.ids.plan, "?tab=keys"), "partners-basic");
  await expect(page.getByTestId("plan-page")).toContainText("partners-basic covers larder-public:prod, and holds 3 API keys. A method that requires an API key is served only with one of them.");
  const details = page.getByTestId("plan-details");
  await expect(details).toContainText("larder-public:prod");
  await expect(details).toContainText("50 requests per second, burst 100");
  await expect(details).toContainText("10,000 requests per month");
  await expect(page.getByTestId("keys-count")).toHaveText("3");
  const keys = page.getByRole("table", { name: "Associated API keys" });
  await expect(keys).toContainText("storefront-berlin");
  await expect(keys).toContainText("storefront-lyon");
  await expect(keys).toContainText("storefront-oslo");
  await expect(page.getByTestId("plan-keys")).toContainText("a request whose key is missing, disabled or not in a plan that covers the stage gets 403 Forbidden");
  await shot(page, "rest-usage-plan");
  // A new key in one step.
  await page.getByTestId("add-key").click();
  await page.getByTestId("key-mode-new").click();
  await page.getByTestId("key-name").fill("storefront-paris");
  await page.getByTestId("key-add-submit").click();
  await expect(page.getByTestId("key-banner")).toContainText("API key storefront-paris added.");
  await expect(page.getByTestId("keys-count")).toHaveText("4");
  expect(cli(["apigateway", "get-usage-plan-keys", "--usage-plan-id", ctx.ids.plan])).toContain("storefront-paris");
  // An existing key: one the plan does not hold yet.
  cli(["apigateway", "create-api-key", "--name", "storefront-nice", "--enabled"]);
  await page.reload();
  await page.getByTestId("add-key").click();
  await expect(page.getByTestId("add-key-dialog")).toContainText("Add an existing key to partners-basic, or create a new one and add it in one step.");
  await choose(page, "key-pick", /storefront-nice/);
  await page.getByTestId("key-add-submit").click();
  await expect(page.getByTestId("keys-count")).toHaveText("5");
  // A key leaves.
  await page.getByTestId("remove-storefront-nice").click();
  await page.getByTestId("plan-remove-submit").click();
  await expect(page.getByTestId("keys-count")).toHaveText("4");
  await expectNoErrorAnswers(page, refusals);
});

test("apigateway__usage-plan-keys-none: a plan with no keys says its limits apply to nobody yet", async ({ page }) => {
  await go(page, apiPath("usageplans", ctx.ids.trial, "?tab=keys"), "partners-trial");
  await expect(page.getByRole("heading", { name: "No API keys" })).toBeVisible();
  await expect(page.getByTestId("plan-keys")).toContainText("No API key is associated with partners-trial, so its limits apply to nobody yet.");
  await expect(page.getByTestId("keys-count")).toHaveText("0");
  await shot(page, "usage-plan-keys-none");
});

test("apigateway: a stage is added to a plan and taken off it, and the plan's limit is enforced on a method that needs a key", async ({ page, refusals }) => {
  await go(page, apiPath("usageplans", ctx.ids.trial, "?tab=stages"), "partners-trial");
  await expect(page.getByTestId("no-stages")).toContainText("covers no stage yet");
  await page.getByTestId("add-stage").click();
  await choose(page, "stage-api", "larder-public");
  await expect(page.getByTestId("stage-pick")).toContainText("prod");
  await page.getByTestId("stage-add-submit").click();
  await expect(page.getByRole("table", { name: "Associated stages" })).toContainText("larder-public");
  expect(cli(["apigateway", "get-usage-plans"])).toContain('"stage": "prod"');
  await page.getByTestId("remove-prod").click();
  await page.getByTestId("plan-remove-submit").click();
  await expect(page.getByTestId("no-stages")).toBeVisible();
  await expectNoErrorAnswers(page, refusals);
  // Enforced: a quota of one request, a method that needs a key, the second request is refused with 429.
  const plan = json<{ id: string }>(["apigateway", "create-usage-plan", "--name", "one-shot", "--quota", "limit=1,period=DAY", "--api-stages", `apiId=${ctx.ids.rest},stage=prod`]);
  const key = json<{ id: string; value: string }>(["apigateway", "create-api-key", "--name", "one-shot-key", "--enabled"]);
  cli(["apigateway", "create-usage-plan-key", "--usage-plan-id", plan.id, "--key-id", key.id, "--key-type", "API_KEY"]);
  cli(["apigateway", "update-method", "--rest-api-id", ctx.ids.rest, "--resource-id", ctx.ids.receipts, "--http-method", "POST", "--patch-operations", "op=replace,path=/apiKeyRequired,value=true"]);
  cli(["apigateway", "create-deployment", "--rest-api-id", ctx.ids.rest, "--stage-name", "prod"]);
  const call = () => page.request.post(`${ctx.server.endpoint}/_aws/execute-api/${ctx.ids.rest}/prod/receipts`, { headers: { "x-api-key": key.value }, data: "{}" });
  expect((await call()).status()).toBe(200);
  expect((await call()).status()).toBe(429);
});

test("apigateway__usage-plans-none: with every plan deleted the list says so", async ({ page, refusals }) => {
  await go(page, "/resources/apigateway/usageplans", "Usage plans");
  for (const name of ["partners-basic", "partners-trial", "partners-premium", "one-shot"]) {
    await page.getByTestId(`delete-${name}`).click();
    await page.getByTestId("plan-delete-submit").click();
    await expect(page.getByTestId("deleted-banner")).toContainText(name);
    await expectNoErrorAnswers(page, refusals);
    await expect(page.getByTestId(`delete-${name}`)).toBeHidden();
  }
  await expect(page.getByRole("heading", { name: "No usage plans" })).toBeVisible();
  await expect(page.getByText("A usage plan sets throttling and quota limits for the API keys that use it.")).toBeVisible();
  await shot(page, "usage-plans-none");
});
