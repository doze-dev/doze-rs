// The HTTP boards of API Gateway (the four-step create, routes, integrations, authorization, CORS, stages, and their empty states), each reached against
// a real doze-aws (one per browser, seeded as the boards' "larder" world) and, in Chromium, photographed light and dark. The REST boards are
// apigateway-boards.spec.ts, the usage plans apigateway-boards-plans.spec.ts.
import { expect, test, CUT } from "./fixtures";
import { apiPath, begin, choose, cli, ctx, end, go, json, pick, shot } from "./apigateway-kit";

test.describe.configure({ mode: "serial" });
test.use({ allowErrors: CUT });

test.beforeAll(async ({ browserName }) => begin(browserName));
test.afterAll(end);

const http = () => ctx.ids.http;
const next = (page: import("@playwright/test").Page) => page.getByTestId("form-submit").click();

test("apigateway__http-create (and its routes, stage and review steps): four steps, then the four calls", async ({ page }) => {
  await go(page, "/resources/apigateway/restapis?do=create-http", "Create HTTP API");
  await expect(page.getByTestId("http-wizard")).toContainText("Step 1 of 4");
  await pick(page, /Choose a function/, "export-report");
  await expect(page.getByTestId("section-integrations")).toContainText("export-report runs python3.13");
  await page.getByTestId("api-name").fill("larder-reports");
  await expect(page.getByTestId("section-api")).toContainText("This Region already has 2 REST APIs, larder-internal and larder-public, and 2 HTTP APIs, larder-admin and larder-hooks.");
  await expect(page.getByTestId("section-api")).toContainText("Stored, not applied locally.");
  await shot(page, "http-create");
  await next(page);
  await expect(page.locator("h1")).toHaveText("Configure routes");
  await expect(page.getByTestId("route-path")).toHaveValue("/export-report");
  await expect(page.getByTestId("section-routes")).toContainText("How routes match.");
  await shot(page, "http-create-routes");
  await next(page);
  await expect(page.locator("h1")).toHaveText("Stage");
  await expect(page.getByTestId("stage-name")).toHaveValue("$default");
  await expect(page.getByTestId("section-stage")).toContainText("This API will answer at https://<api-id>.execute-api.us-east-1.amazonaws.com/export-report.");
  await shot(page, "http-create-stages");
  await next(page);
  await expect(page.locator("h1")).toHaveText("Review and create");
  const summary = page.getByTestId("section-summary");
  await expect(summary).toContainText("larder-reports");
  await expect(summary).toContainText("ANY /export-report");
  const code = page.getByTestId("call-panel-code");
  await expect(code).toHaveCount(4);
  await expect(code.nth(0)).toContainText('"protocolType": "HTTP"');
  await expect(code.nth(1)).toContainText('"integrationType": "AWS_PROXY"');
  await expect(code.nth(2)).toContainText('"routeKey": "ANY /export-report"');
  await expect(code.nth(3)).toContainText('"autoDeploy": true');
  await expect(page.getByTestId("call-panel")).toContainText("Each call also carries the new API's ID.");
  await shot(page, "http-create-review");
  await page.getByTestId("edit-routes").click();
  await expect(page.locator("h1")).toHaveText("Configure routes");
  await next(page);
  await next(page);
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("HTTP API larder-reports created.");
  await expect(page.locator("h1")).toHaveText("Routes");
  const made = json<{ Items: { Name: string; ApiId: string }[] }>(["apigatewayv2", "get-apis"]).Items.find((a) => a.Name === "larder-reports")!;
  expect(cli(["apigatewayv2", "get-routes", "--api-id", made.ApiId])).toContain("ANY /export-report");
  expect(cli(["apigatewayv2", "get-stages", "--api-id", made.ApiId])).toContain("$default");
});

test("apigateway__http-routes: the routes with their integration and authorization, the one chosen, and how a request is matched", async ({ page }) => {
  await go(page, apiPath("apis", http()), "Routes");
  await expect(page.getByTestId("chip-kind")).toHaveText("HTTP");
  const sentence = page.getByTestId("list-sentence");
  await expect(sentence).toContainText("larder-admin has 3 routes on 2 stages, $default and beta. 2 routes run export-report. ANY /orders/{proxy+} has no integration yet.");
  const table = page.getByRole("table", { name: "Routes" });
  await expect(table).toContainText("GET /reports");
  await expect(table).toContainText("CUSTOM");
  await page.getByTestId("route-key").filter({ hasText: "ANY /orders" }).click();
  const selected = page.getByTestId("selected-route");
  await expect(selected).toContainText(ctx.ids.ordersRoute);
  await expect(selected).toContainText("No integration is attached to this route. Attach one on the Integrations page.");
  await expect(selected).toContainText("NONE: no authorizer, the route is open.");
  await expect(selected).toContainText("POST /orders/12 would land on this route.");
  await expect(page.getByTestId("recent-calls")).toContainText("CreateRoute");
  await shot(page, "http-routes");
});

test("apigateway__http-route-integration: attach an integration to a route, the call, and the integrations that exist", async ({ page }) => {
  await go(page, apiPath("apis", http(), "?tab=integrations"), "Integrations");
  await page.getByTestId("route-key").filter({ hasText: "ANY /orders" }).click();
  await expect(page.getByTestId("attach")).toContainText("Attach an integration to ANY /orders/{proxy+}");
  await choose(page, "attach-pick", /orders\.larder\.example\.com/);
  await expect(page.getByTestId("attach")).toContainText("HTTP proxy to https://orders.larder.example.com/{proxy}. A {param} placeholder is filled from the request path");
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText("UpdateRoute");
  await expect(call).toContainText(`"RouteId": "${ctx.ids.ordersRoute}"`);
  await expect(call).toContainText(`"target": "integrations/${ctx.ids.httpIntegration}"`);
  const existing = page.getByRole("table", { name: "Existing integrations" });
  await expect(existing).toContainText("AWS_PROXY");
  await expect(existing).toContainText("function:export-report");
  await expect(existing).toContainText("HTTP_PROXY");
  await shot(page, "http-route-integration");
  await page.getByTestId("attach-submit").click();
  await expect(page.getByTestId("attach-done")).toContainText("ANY /orders/{proxy+} now calls");
  expect(cli(["apigatewayv2", "get-route", "--api-id", http(), "--route-id", ctx.ids.ordersRoute])).toContain(`integrations/${ctx.ids.httpIntegration}`);
});

test("apigateway__http-authorizer-create: a Lambda authorizer, the call, and attaching it to a route as the second step", async ({ page }) => {
  await go(page, apiPath("apis", http(), "?tab=authorization&mode=manage"), "Authorization");
  await expect(page.getByTestId("authorizer-name")).toContainText("admin-request-auth");
  await page.getByTestId("create-authorizer").click();
  await expect(page.locator("h1")).toHaveText("Create authorizer");
  await expect(page.getByTestId("kind-jwt")).toBeEnabled();
  await page.getByTestId("authorizer-name-input").fill("orders-request-auth");
  await pick(page, /Choose a function/, "admin-auth");
  await expect(page.getByTestId("authorizer-simple")).toBeChecked();
  await expect(page.getByTestId("authorizer-sources-item")).toContainText("$request.header.Authorization");
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText("CreateAuthorizer");
  await expect(call).toContainText('"authorizerType": "REQUEST"');
  await expect(call).toContainText('"authorizerPayloadFormatVersion": "2.0"');
  await expect(call).toContainText('"authorizerResultTtlInSeconds": 300');
  await expect(call).toContainText("Attaching it to a route is a second step, on the Authorization page");
  await shot(page, "http-authorizer-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("authorizer-banner")).toContainText("Authorizer orders-request-auth created.");
  // The second step: the route is attached, and its type becomes CUSTOM.
  await page.getByTestId("tab-attach").click();
  await choose(page, "attach-route", /ANY \/orders\/\{proxy\+\}/);
  await choose(page, "attach-authorizer-pick", "orders-request-auth");
  await expect(page.getByTestId("attach-authorizer")).toContainText("The route's authorization type becomes CUSTOM.");
  await page.getByTestId("attach-submit").click();
  await expect(page.getByTestId("attach-done")).toContainText("is now checked by orders-request-auth");
  expect(cli(["apigatewayv2", "get-route", "--api-id", http(), "--route-id", ctx.ids.ordersRoute])).toContain("CUSTOM");
});

test("apigateway__http-authorizer-create: a JWT authorizer is built, not refused", async ({ page }) => {
  await go(page, apiPath("apis", http(), "?do=create-authorizer"), "Create authorizer");
  await page.getByTestId("kind-jwt").click();
  await page.getByTestId("authorizer-name-input").fill("admin-jwt");
  await page.getByTestId("authorizer-issuer").fill("https://idp.larder.example.com/");
  await page.getByTestId("authorizer-audience").fill("larder-admin");
  await expect(page.getByTestId("call-panel")).toContainText('"authorizerType": "JWT"');
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("authorizer-banner")).toContainText("Authorizer admin-jwt created.");
  expect(cli(["apigatewayv2", "get-authorizers", "--api-id", http()])).toContain("admin-jwt");
});

test("apigateway__http-cors: the origins, headers, methods and max age, saved with UpdateApi", async ({ page }) => {
  await go(page, apiPath("apis", http(), "?tab=cors"), "Configure CORS");
  await expect(page.getByTestId("chip-kind")).toHaveText("HTTP");
  await expect(page.getByTestId("cors-form")).toContainText("larder-admin has GET /reports, POST /reports/run and ANY /orders/{proxy+}.");
  const add = async (id: string, values: string[]) => { for (const v of values) { await page.getByTestId(`${id}-input`).fill(v); await page.getByTestId(`${id}-add`).click(); } };
  await add("cors-origins", ["https://admin.larder.example.com"]);
  await add("cors-headers", ["authorization", "content-type"]);
  await add("cors-methods", ["GET", "POST"]);
  await add("cors-expose", ["x-api-id"]);
  await page.getByTestId("cors-max-age").fill("300");
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText('"allowOrigins": [');
  await expect(call).toContainText('"maxAge": 300');
  await expect(call).toContainText('"allowCredentials": false');
  await expect(page.getByTestId("cors-credentials-note")).toContainText("Off. The credentials header is not sent.");
  await shot(page, "http-cors");
  await page.getByTestId("cors-save").click();
  await expect(page.getByTestId("cors-banner")).toContainText("CORS configuration saved.");
  const api = json<{ CorsConfiguration: { AllowOrigins: string[]; MaxAge: number } }>(["apigatewayv2", "get-api", "--api-id", http()]);
  expect(api.CorsConfiguration).toMatchObject({ AllowOrigins: ["https://admin.larder.example.com"], MaxAge: 300 });
  // A preflight is answered by the API itself.
  const res = await page.request.fetch(`${ctx.server.endpoint}/_aws/execute-api/${http()}/reports`, { method: "OPTIONS", headers: { origin: "https://admin.larder.example.com", "access-control-request-method": "GET" } });
  expect(res.status()).toBe(204);
});

test("apigateway__http-stages: the stages, the invoke URL in both forms, the deployment, and Deploy for a stage deployed by hand", async ({ page }) => {
  await go(page, apiPath("apis", http(), "?tab=stages"), "Stages");
  const sentence = page.getByTestId("list-sentence");
  await expect(sentence).toContainText("$default releases every change by itself; its last deployment,");
  await expect(sentence).toContainText("beta is deployed by hand and has no deployment yet.");
  const table = page.getByRole("table", { name: "Stages" });
  await expect(table).toContainText("On");
  await expect(table).toContainText("Off");
  const details = page.getByTestId("stage-details");
  await expect(details).toContainText(`https://${http()}.execute-api.us-east-1.amazonaws.com`);
  await expect(details).not.toContainText("/$default");
  await expect(details).toContainText(`/_aws/execute-api/${http()}`);
  await expect(details).toContainText("automatic");
  await expect(details).toContainText("Production");
  await expect(details).toContainText("The $default stage is served from the base of the URL, with no stage name in the path; beta answers under /beta.");
  await shot(page, "http-stages");
  await page.getByTestId("stage-name-cell").filter({ hasText: "beta" }).click();
  await page.getByTestId("deploy-stage").click();
  await expect(details).toContainText("Next release");
  await expect(page.getByRole("table", { name: "Stages" })).toContainText("beta");
  expect(cli(["apigatewayv2", "get-stages", "--api-id", http()])).toMatch(/"StageName": "beta"/);
  await expect(page.getByTestId("list-sentence")).toContainText("beta is deployed by hand and points at");
});

test("apigateway__http-stages: a stage is made and edited", async ({ page }) => {
  await go(page, apiPath("apis", http(), "?tab=stages"), "Stages");
  await page.getByTestId("create-stage").click();
  await page.getByTestId("http-stage-name").fill("canary");
  await page.getByTestId("http-stage-description").fill("Ten percent");
  await page.getByTestId("http-stage-save").click();
  await expect(page.getByRole("table", { name: "Stages" })).toContainText("canary");
  await page.getByTestId("stage-name-cell").filter({ hasText: "canary" }).click();
  await page.getByTestId("edit-stage").click();
  await page.getByTestId("http-stage-auto").click();
  await page.getByTestId("http-stage-save").click();
  await expect(page.getByTestId("stage-details")).toContainText("Automatic deployment");
  expect(cli(["apigatewayv2", "get-stage", "--api-id", http(), "--stage-name", "canary"])).toContain('"AutoDeploy": true');
});

test("apigateway__http-routes-none, http-integrations-none and http-stages-none: an API with nothing says what each is", async ({ page }) => {
  await go(page, apiPath("apis", ctx.ids.hooks), "Routes");
  await expect(page.getByRole("heading", { name: "No routes" })).toBeVisible();
  await expect(page.getByText("A route maps a request method and path to an integration.")).toBeVisible();
  await expect(page.getByTestId("create-route-top")).toBeVisible();
  await shot(page, "http-routes-none");
  await go(page, apiPath("apis", ctx.ids.hooks, "?tab=integrations"), "Integrations");
  await expect(page.getByRole("heading", { name: "No integrations" })).toBeVisible();
  await expect(page.getByTestId("http-page")).toContainText("larder-hooks has no integrations. An integration connects a route to a backend such as a Lambda function or an HTTP endpoint.");
  await shot(page, "http-integrations-none");
  await go(page, apiPath("apis", ctx.ids.hooks, "?tab=stages"), "Stages");
  await expect(page.getByRole("heading", { name: "No stages" })).toBeVisible();
  await expect(page.getByTestId("http-page")).toContainText("larder-hooks has no stage, so it cannot be invoked. A stage is a named reference to a deployment.");
  await shot(page, "http-stages-none");
});

test("apigateway: a route and an integration are made on the pages, and a WebSocket API is built", async ({ page }) => {
  await go(page, apiPath("apis", ctx.ids.hooks), "Routes");
  await page.getByTestId("create-route-top").click();
  await choose(page, "route-method", "POST");
  await page.getByTestId("route-path").fill("/github");
  await expect(page.getByTestId("call-panel")).toContainText('"routeKey": "POST /github"');
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("route-banner")).toContainText("Route POST /github created.");
  await go(page, apiPath("apis", ctx.ids.hooks, "?do=create-integration"), "Create integration");
  await pick(page, /Choose a function/, "export-report");
  await choose(page, "integration-route", "POST /github");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("integration-banner")).toContainText("created.");
  expect(cli(["apigatewayv2", "get-routes", "--api-id", ctx.ids.hooks])).toContain("integrations/");
  await go(page, "/resources/apigateway/restapis?do=create-websocket", "Create WebSocket API");
  await page.getByTestId("api-name").fill("larder-live");
  await expect(page.getByTestId("call-panel")).toContainText('"protocolType": "WEBSOCKET"');
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("created-banner")).toContainText("WebSocket API larder-live created.");
  await expect(page.getByTestId("chip-kind")).toHaveText("WebSocket");
});

test("apigateway__apis-none: with every API deleted the list says so, and a route is deleted from its row", async ({ page }) => {
  await go(page, apiPath("apis", http()), "Routes");
  await page.getByTestId("delete-GET /reports").click();
  await page.getByTestId("route-delete-submit").click();
  await expect(page.getByTestId("delete-GET /reports")).toBeHidden();
  for (const a of json<{ Items: { ApiId: string }[] }>(["apigatewayv2", "get-apis"]).Items) cli(["apigatewayv2", "delete-api", "--api-id", a.ApiId]);
  for (const a of json<{ items: { id: string }[] }>(["apigateway", "get-rest-apis"]).items) cli(["apigateway", "delete-rest-api", "--rest-api-id", a.id]);
  await go(page, "/resources/apigateway/restapis", "APIs");
  await expect(page.getByRole("heading", { name: "No APIs" })).toBeVisible();
  await expect(page.getByText("You have no APIs in this region. Create a REST or HTTP API to expose a backend.")).toBeVisible();
  await shot(page, "apis-none");
});
