// The REST boards of API Gateway (the list, the type chooser, create, resources, methods, deploy, stages, authorizers), each reached against a real
// doze-aws (one per browser, seeded as the boards' "larder" world with the real AWS CLI) and, in Chromium, photographed light and dark into SHOTS_DIR to
// be set beside the board. The HTTP boards and the usage plans are apigateway-boards-http.spec.ts and apigateway-boards-plans.spec.ts.
import { expect, expectNoErrorAnswers, test } from "./fixtures";
import { apiPath, begin, choose, cli, ctx, end, go, json, pick, shot } from "./apigateway-kit";

test.describe.configure({ mode: "serial" });

test.beforeAll(async ({ browserName }) => begin(browserName));
test.afterAll(end);

const rest = () => ctx.ids.rest;
const methodPath = (resource: string, method: string, mode = "") => apiPath("restapis", rest(), `?do=method&key=${resource}&sel=${method}${mode ? `&mode=${mode}` : ""}`);

test("apigateway__apis-list: both kinds of API in one list, the sentence over it, the account's navigation, and the footer count", async ({ page }) => {
  await go(page, "/resources/apigateway/restapis", "APIs");
  const sentence = page.getByTestId("list-sentence");
  await expect(sentence).toContainText("larder-public answers through its prod stage, deployed");
  await expect(sentence).toContainText("larder-admin answers at its $default stage, which deploys every change you save.");
  await expect(page.getByTestId("account-nav")).toContainText("Usage plans");
  await expect(page.getByRole("link", { name: "larder-public" })).toBeVisible();
  await expect(page.getByRole("link", { name: "larder-admin" })).toBeVisible();
  await expect(page.getByText("1 to 4 of 4 APIs")).toBeVisible();
  await expect(page.locator("tbody tr", { hasText: "larder-internal" })).toContainText("Edge-optimized");
  await expect(page.locator("tbody tr", { hasText: "larder-public" })).toContainText("Regional");
  await page.getByTestId(`select-${ctx.ids.http}`).click();
  await page.getByTestId("list-search").fill("larder-");
  await shot(page, "apis-list");
  await page.getByTestId("list-search").fill("");
});

test("apigateway__http-delete: Actions then Delete names the API with what goes with it, and a plain confirm", async ({ page }) => {
  await go(page, "/resources/apigateway/restapis", "APIs");
  await page.getByTestId(`select-${ctx.ids.hooks}`).click();
  await page.getByTestId("list-actions").click();
  await page.getByTestId("action-delete").click();
  const dialog = page.getByTestId("api-delete");
  await expect(dialog).toContainText("Delete larder-hooks");
  await expect(dialog.getByTestId("delete-impact")).toContainText("0 routes");
  await page.getByRole("button", { name: "Cancel" }).click();
  await page.getByTestId(`select-${ctx.ids.hooks}`).click();
  await page.getByTestId(`select-${ctx.ids.http}`).click();
  await page.getByTestId("list-actions").click();
  await page.getByTestId("action-delete").click();
  await expect(dialog).toContainText("Delete larder-admin (");
  await expect(dialog.getByTestId("delete-impact")).toContainText("3 routes, the integrations behind them, the Lambda authorizer and the $default and beta stages go with it.");
  await shot(page, "http-delete");
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
});

test("apigateway__api-create-type: HTTP, REST and WebSocket as cards, and how they differ here", async ({ page }) => {
  await go(page, "/resources/apigateway/restapis", "APIs");
  await page.getByTestId("create-open").click();
  await expect(page.locator("h1")).toHaveText("Create API");
  await expect(page.getByTestId("type-websocket")).toContainText("route selection expression");
  await expect(page.getByTestId("type-websocket")).not.toContainText("Not available");
  await expect(page.getByTestId("expect-table")).toContainText("Lambda (request), JWT");
  await expect(page.getByTestId("expect-table")).toContainText("Yes, enforced here");
  await shot(page, "api-create-type");
});

test("apigateway__rest-create: Regional, the IP address type, the call beside it, and a landing on the new API's Resources", async ({ page }) => {
  await go(page, "/resources/apigateway/restapis?do=create", "Create API");
  await page.getByTestId("build-rest").click();
  await expect(page.locator("h1")).toHaveText("Create REST API");
  await expect(page.getByTestId("section-details")).toContainText("This Region already has 2 REST APIs, larder-internal and larder-public, and 2 HTTP APIs, larder-admin and larder-hooks.");
  await expect(page.getByTestId("endpoint-edge")).toBeDisabled();
  await page.getByTestId("api-name").fill("larder-orders");
  await page.getByTestId("api-description").fill("Order intake for partner storefronts");
  await expect(page.getByTestId("section-details")).toContainText("Stored, not applied locally.");
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText("CreateRestApi");
  await expect(call).toContainText('"types": [');
  await expect(call).toContainText('"ipAddressType": "ipv4"');
  await shot(page, "rest-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("REST API larder-orders created.");
  await expect(page.locator("h1")).toHaveText("Resources");
  expect(JSON.parse(cli(["apigateway", "get-rest-apis"])).items.find((a: { name: string }) => a.name === "larder-orders").endpointConfiguration.types).toEqual(["REGIONAL"]);
});

test("apigateway__rest-resources: the tree with its methods, the methods of one, and the API's recent calls", async ({ page }) => {
  await go(page, apiPath("restapis", rest()), "Resources");
  await expect(page.getByTestId("rest-page")).toContainText("larder-public has 3 resources and 3 methods, and one stage: prod, pointing at deployment");
  await expect(page.getByTestId("chip-name")).toHaveText("larder-public");
  await expect(page.getByTestId("chip-kind")).toHaveText("REST");
  await expect(page.getByTestId("chip-extra")).toHaveText("Regional");
  await expect(page.getByTestId("resource-tree")).toContainText("/products");
  await expect(page.getByTestId("resource-tree")).toContainText("/receipts");
  await page.getByTestId("resource-/products").click();
  const methods = page.getByRole("table", { name: "Methods of /products" });
  await expect(methods).toContainText("GET");
  await expect(methods).toContainText("HTTP");
  await expect(methods).toContainText("OPTIONS");
  await expect(methods).toContainText("Mock");
  await expect(methods).toContainText("Not required");
  await expect(page.getByTestId("recent-calls")).toContainText("CreateDeployment");
  await shot(page, "rest-resources");
});

test("apigateway__rest-resource-create: a path parameter under /products with CORS, the call, and the OPTIONS method it makes", async ({ page }) => {
  await go(page, apiPath("restapis", rest()), "Resources");
  await page.getByTestId("resource-/products").click();
  await page.getByTestId("create-resource").click();
  await expect(page.locator("h1")).toHaveText("Create resource");
  await page.getByTestId("resource-name").fill("{sku}");
  await expect(page.getByTestId("section-details")).toContainText("/products has a GET and an OPTIONS method");
  await page.getByTestId("resource-cors").click();
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText("CreateResource");
  await expect(call).toContainText('"pathPart": "{sku}"');
  await expect(call).toContainText("PutIntegrationResponse");
  await page.getByTestId("resource-cors").click();
  await shot(page, "rest-resource-create");
  await page.getByTestId("resource-cors").click();
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("resource-banner")).toContainText("Resource /products/{sku} created.");
  await expect(page.getByTestId("resource-tree")).toContainText("/products/{sku}");
  const tree = json<{ items: { path: string; resourceMethods?: Record<string, unknown> }[] }>(["apigateway", "get-resources", "--rest-api-id", rest(), "--embed", "methods"]);
  expect(Object.keys(tree.items.find((r) => r.path === "/products/{sku}")?.resourceMethods ?? {})).toEqual(["OPTIONS"]);
});

test("apigateway__rest-method-create: a Lambda proxy method, with the two calls it makes", async ({ page }) => {
  await go(page, apiPath("restapis", rest()), "Resources");
  await page.getByTestId("resource-/products").click();
  await page.getByTestId("create-method").click();
  await expect(page.locator("h1")).toHaveText("Create method");
  await expect(page.getByTestId("section-details")).toContainText("/products already has GET and OPTIONS");
  // It opens on a verb the resource has not got (the resource is read after the form opens), so the form is not refused from the first look.
  await expect(page.getByTestId("method-type")).toHaveText("POST");
  await expect(page.getByTestId("section-details")).not.toContainText("already has a GET method");
  await choose(page, "method-type", "POST");
  await pick(page, /Choose a function/, "resize-product-image");
  await expect(page.getByTestId("section-details")).toContainText("Lambda proxy integration");
  await expect(page.getByTestId("section-settings-summary")).toContainText("Authorization None, API key not required.");
  const code = page.getByTestId("call-panel-code");
  await expect(code.first()).toContainText("PutMethod");
  await expect(code.nth(1)).toContainText("PutIntegration");
  await expect(code.nth(1)).toContainText('"integrationHttpMethod": "POST"');
  await expect(code.nth(1)).toContainText('"type": "AWS_PROXY"');
  await expect(code.nth(1)).toContainText("function:resize-product-image/invocations");
  await shot(page, "rest-method-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("method-banner")).toContainText("Method POST /products created.");
  await expect(page.getByRole("table", { name: "Methods of /products" })).toContainText("Lambda function");
  expect(cli(["apigateway", "get-method", "--rest-api-id", rest(), "--resource-id", ctx.ids.products, "--http-method", "POST"])).toContain("AWS_PROXY");
});

test("apigateway__rest-method-request: the request path, the settings, the query strings the method accepts", async ({ page }) => {
  await go(page, methodPath(ctx.ids.products, "GET"), "GET /products");
  await expect(page.getByTestId("integration-badge")).toHaveText("HTTP integration");
  await expect(page.getByTestId("method-page")).toContainText("A GET to /products needs no authorization and no API key. It accepts two query strings, category and page, and the catalog.larder.example.com backend answers.");
  await expect(page.getByTestId("wired-strip")).toContainText("Request path");
  await expect(page.getByTestId("method-settings")).toContainText("ListProducts");
  await expect(page.getByTestId("method-settings")).toContainText(`arn:aws:execute-api:us-east-1:${ctx.server.account}:${rest()}/*/GET/products`);
  await expect(page.getByTestId("method-settings")).toContainText("larder-public has one, larder-token-auth; this method does not use it.");
  await expect(page.getByTestId("method-settings")).toContainText("partners-basic covers larder-public, stage prod.");
  await expect(page.getByTestId("method-queries")).toContainText("category");
  await expect(page.getByTestId("method-headers")).toContainText("No headers are declared on this method.");
  await shot(page, "rest-method-request");
});

test("apigateway__rest-integration-request: what the backend is sent, the mappings doze-aws applies, and the templates", async ({ page }) => {
  await go(page, methodPath(ctx.ids.products, "GET", "integration"), "GET /products");
  const settings = page.getByTestId("integration-settings");
  await expect(settings).toContainText("https://catalog.larder.example.com/v1/products");
  await expect(settings).toContainText("On (29 seconds)");
  await expect(settings).toContainText("Stored, not applied locally: bodies are passed to the backend as they arrive.");
  await expect(page.getByTestId("integration-queries")).toContainText("method.request.querystring.category");
  await expect(page.getByTestId("integration-queries")).toContainText("doze-aws applies these mappings");
  await expect(page.getByTestId("integration-paths")).toContainText("None are defined.");
  await expect(page.getByTestId("integration-templates")).toContainText("runs Velocity templates");
  await shot(page, "rest-integration-request");
});

test("apigateway__rest-integration-request, Edit: the timeout, a mapping template and a parameter mapping are changed on the page, and put back", async ({ page }) => {
  await go(page, methodPath(ctx.ids.products, "GET", "integration"), "GET /products");
  const integration = () => json<{ timeoutInMillis: number; requestTemplates?: Record<string, string>; requestParameters: Record<string, string> }>(["apigateway", "get-integration", "--rest-api-id", rest(), "--resource-id", ctx.ids.products, "--http-method", "GET"]);
  await page.getByTestId("edit-integration").click();
  const dialog = page.getByTestId("integration-dialog");
  await dialog.getByTestId("timeout-default").click();
  await dialog.getByTestId("timeout-ms").fill("20");
  await dialog.getByTestId("integration-save").click();
  await expect(dialog).toContainText("A custom timeout is 50 to 29,000 milliseconds.");
  await dialog.getByTestId("timeout-ms").fill("5000");
  await dialog.getByTestId("add-template").click();
  await dialog.getByTestId("template-type-input").fill("application/json");
  await dialog.getByTestId("template-text-input").fill('{"category": "$input.params(\'category\')"}');
  await dialog.getByTestId("add-param").click();
  await dialog.getByTestId("param-where").last().click();
  await page.getByRole("option", { name: "Header" }).click();
  await dialog.getByTestId("param-name").last().fill("X-Source");
  await dialog.getByTestId("param-from").last().fill("method.request.querystring.page");
  await dialog.getByTestId("integration-save").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("integration-settings")).toContainText("Custom (5,000 ms)");
  await expect(page.getByTestId("integration-templates")).toContainText("application/json");
  await expect(page.getByTestId("integration-headers")).toContainText("X-Source");
  const edited = integration();
  expect(edited.timeoutInMillis).toBe(5000);
  expect(edited.requestTemplates?.["application/json"]).toContain("category");
  expect(edited.requestParameters["integration.request.header.X-Source"]).toBe("method.request.querystring.page");
  expect(edited.requestParameters["integration.request.querystring.category"]).toBe("method.request.querystring.category");
  // Put back: the default timeout, no template, no extra header; what the seed made is kept.
  await page.getByTestId("edit-integration").click();
  await dialog.getByTestId("timeout-default").click();
  await dialog.getByRole("button", { name: "Remove template" }).click();
  await dialog.getByRole("button", { name: "Remove parameter" }).last().click();
  await dialog.getByTestId("integration-save").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("integration-settings")).toContainText("On (29 seconds)");
  const back = integration();
  expect(back.timeoutInMillis).toBe(29000);
  expect(back.requestTemplates ?? {}).toEqual({});
  expect(Object.keys(back.requestParameters).sort()).toEqual(["integration.request.querystring.category", "integration.request.querystring.page"]);
});

test("apigateway__rest-integration-response: the default response, its regex, and a response made on the page", async ({ page }) => {
  await go(page, methodPath(ctx.ids.products, "GET", "iresponse"), "GET /products");
  await expect(page.getByTestId("wired-strip")).toContainText("Response path");
  const first = page.getByTestId("integration-response").first();
  await expect(first).toContainText("Default - Response");
  await expect(first).toContainText("Empty: this is the default response");
  await expect(page.getByTestId("integration-mappings")).toContainText("the backend's answer is returned as it is");
  await shot(page, "rest-integration-response");
  cli(["apigateway", "put-method-response", "--rest-api-id", rest(), "--resource-id", ctx.ids.products, "--http-method", "GET", "--status-code", "400"]);
  await page.reload();
  await page.getByTestId("create-response").click();
  await choose(page, "response-status", "400");
  await page.getByTestId("response-pattern").fill("4\\d\\d");
  await expect(page.getByTestId("integration-response-dialog")).toContainText("PutIntegrationResponse");
  await page.getByTestId("integration-response-dialog-submit").click();
  await expect(page.getByTestId("integration-response")).toHaveCount(2);
  expect(cli(["apigateway", "get-integration-response", "--rest-api-id", rest(), "--resource-id", ctx.ids.products, "--http-method", "GET", "--status-code", "400"])).toContain("4\\\\d\\\\d");
});

test("apigateway__rest-method-response: the status codes the method can return, and what a missing one does here", async ({ page }) => {
  await go(page, methodPath(ctx.ids.products, "GET", "mresponse"), "GET /products");
  const first = page.getByTestId("method-response").first();
  await expect(first).toContainText("200");
  await expect(first).toContainText("application/json");
  await expect(first).toContainText("Empty");
  await expect(page.getByTestId("method-response-help")).toContainText("a missing method response is not a 500 here");
  await shot(page, "rest-method-response");
  await page.getByTestId("create-response").click();
  await page.getByTestId("response-status").fill("404");
  await page.getByTestId("response-headers").fill("X-Why");
  await page.getByTestId("method-response-dialog-submit").click();
  await expect(page.getByTestId("method-response")).toHaveCount(3);
  expect(cli(["apigateway", "get-method-response", "--rest-api-id", rest(), "--resource-id", ctx.ids.products, "--http-method", "GET", "--status-code", "404"])).toContain("X-Why");
});

test("apigateway__rest-deploy: Deploy API to a stage, and the stage serves what was deployed", async ({ page }) => {
  await go(page, apiPath("restapis", rest()), "Resources");
  await page.getByTestId("deploy-open").click();
  const dialog = page.getByTestId("deploy-dialog");
  await expect(dialog).toContainText("Create a deployment of larder-public and point a stage at it.");
  await expect(dialog).toContainText("larder-public has one stage, prod.");
  await expect(page.getByTestId("deploy-stage")).toContainText("prod");
  await dialog.getByTestId("deploy-description").fill("Add POST /products");
  await expect(dialog).toContainText("a change to a resource or a method shows on the stage only after you deploy");
  await shot(page, "rest-deploy");
  await dialog.getByTestId("deploy-submit").click();
  await expect(page.getByTestId("deployed-banner")).toContainText("Deployed to prod.");
  await expect(page.locator("h1")).toHaveText("Stages");
  expect(cli(["apigateway", "get-deployments", "--rest-api-id", rest()])).toContain("Add POST /products");
});

test("apigateway__rest-stage: the stage's details, logs and variables, each edited with UpdateStage, and the deployment history", async ({ page }) => {
  await go(page, apiPath("restapis", rest(), "?tab=stages"), "Stages");
  await expect(page.getByTestId("rest-page")).toContainText("Stage prod points at deployment");
  await expect(page.getByTestId("rest-page")).toContainText("with the description “Add POST /products”.");
  const details = page.getByTestId("stage-details");
  await expect(details).toContainText(`https://${rest()}.execute-api.us-east-1.amazonaws.com/prod`);
  await expect(details).toContainText(`/_aws/execute-api/${rest()}/prod`);
  await expect(details).toContainText("Not available in doze-aws: there is no AWS WAF locally.");
  const logs = page.getByTestId("stage-logging");
  await expect(logs).toContainText("Errors only");
  await expect(logs).toContainText("arn:aws:logs:us-east-1:");
  await expect(logs).toContainText("Not available in doze-aws: there is no X-Ray locally.");
  await expect(page.getByTestId("stage-variables-card")).toContainText("catalogHost");
  await shot(page, "rest-stage");
  // Details: a description.
  await page.getByTestId("edit-details").click();
  await page.getByTestId("stage-description").fill("Production, in front of the catalog");
  await page.getByTestId("stage-edit-details-save").click();
  await expect(details).toContainText("Production, in front of the catalog");
  // Logs: the level and data tracing.
  await page.getByTestId("edit-logging").click();
  await page.getByTestId("stage-level-INFO").click();
  await page.getByTestId("stage-edit-logging-save").click();
  await expect(logs).toContainText("Errors and info logs");
  // Variables: one changed, one added.
  await page.getByTestId("edit-variables").click();
  await page.getByTestId("stage-variables-key-0").fill("catalogHost");
  await page.getByTestId("stage-variables-value-1").fill("512");
  await page.getByRole("button", { name: "Add variable" }).click();
  await page.getByTestId("stage-variables-key-2").fill("region");
  await page.getByTestId("stage-variables-value-2").fill("eu");
  await page.getByTestId("stage-edit-variables-save").click();
  await expect(page.getByTestId("stage-variables-card")).toContainText("region");
  const stage = json<{ variables: Record<string, string>; description: string; methodSettings: Record<string, { loggingLevel: string }> }>(["apigateway", "get-stage", "--rest-api-id", rest(), "--stage-name", "prod"]);
  expect(stage.variables).toMatchObject({ thumbSize: "512", region: "eu" });
  expect(stage.methodSettings["*/*"]?.loggingLevel).toBe("INFO");
  await page.getByTestId("tab-history").click();
  await expect(page.getByTestId("stage-history-card")).toContainText("Add POST /products");
  await expect(page.getByTestId("stage-history-card")).toContainText("Active");
});

test("apigateway__rest-stage: a second stage is made from a deployment of the history", async ({ page }) => {
  await go(page, apiPath("restapis", rest(), "?tab=stages"), "Stages");
  await page.getByTestId("create-stage").click();
  await expect(page.locator("h1")).toHaveText("Create stage");
  await page.getByTestId("stage-name").fill("beta");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("stage-banner")).toContainText("Stage beta created.");
  await expect(page.getByTestId("stage-chip-beta")).toBeVisible();
});

test("apigateway__rest-delete: a typed confirm that names the API with what goes with it, and the usage plan that lists it", async ({ page, refusals }) => {
  await go(page, apiPath("restapis", rest()), "Resources");
  await page.getByTestId("api-actions").click();
  await page.getByTestId("action-delete-api").click();
  const dialog = page.getByTestId("api-delete");
  await expect(dialog).toContainText("Delete larder-public (");
  await expect(dialog.getByTestId("delete-impact")).toContainText("4 resources, 5 methods");
  await expect(dialog.getByTestId("delete-impact")).toContainText("prod and beta");
  // Who called it lately is not known to the record (a request to the API's address is unsigned): the dialog says so.
  await expect(dialog.getByTestId("delete-callers")).toContainText("Who called this API lately is not shown");
  await expect(dialog.getByTestId("delete-plans")).toContainText("partners-basic");
  await expect(dialog.getByTestId("delete-plans")).toContainText("larder-public:prod");
  await expect(dialog.getByTestId("api-delete-submit")).toBeDisabled();
  await shot(page, "rest-delete");
  await dialog.getByTestId("api-delete-input").fill("confirm");
  await dialog.getByTestId("api-delete-submit").click();
  await expect(page.getByTestId("deleted-banner")).toContainText("Deleted larder-public");
  await expectNoErrorAnswers(page, refusals);
  await expect(page.getByRole("link", { name: "larder-public" })).toBeHidden();
  expect(cli(["apigateway", "get-usage-plans"])).not.toContain(rest());
});
