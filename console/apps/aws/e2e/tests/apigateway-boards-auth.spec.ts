// The REST boards of the stages never deployed and of the authorizers (a token authorizer made, listed, deleted to none), each reached against a real doze-aws
// (one per browser, seeded as the boards' "larder" world) and, in Chromium, photographed light and dark. The rest of the REST boards are apigateway-boards.spec.ts.
import { expect, test, CUT } from "./fixtures";
import { apiPath, begin, ctx, end, go, pick, shot } from "./apigateway-kit";

test.describe.configure({ mode: "serial" });
// A few boards are the service's refusal: the browser logs each 4xx as a failed load.
test.use({ allowErrors: CUT });

test.beforeAll(async ({ browserName }) => begin(browserName));
test.afterAll(end);

const rest = () => ctx.ids.rest;

test("apigateway__rest-stages-none: an API never deployed has no stages, and says where to deploy", async ({ page }) => {
  await go(page, apiPath("restapis", ctx.ids.internal, "?tab=stages"), "Stages");
  await expect(page.getByRole("heading", { name: "No stages" })).toBeVisible();
  await expect(page.getByTestId("rest-page")).toContainText("larder-internal has never been deployed, so it has no stages. Deploy the API to create one.");
  await shot(page, "rest-stages-none");
});

test("apigateway__rest-authorizer-create: a token authorizer, the call it makes, and the REQUEST it becomes", async ({ page }) => {
  await go(page, apiPath("restapis", rest(), "?tab=authorizers"), "Authorizers");
  await page.getByTestId("create-authorizer-top").click();
  await expect(page.locator("h1")).toHaveText("Create authorizer");
  await page.getByTestId("authorizer-name-input").fill("partner-token-auth");
  await expect(page.getByTestId("section-details")).toContainText("larder-token-auth");
  await expect(page.getByTestId("kind-cognito")).toBeEnabled();
  await pick(page, /Choose a function/, "token-auth");
  await page.getByTestId("authorizer-validation").fill("^Bearer [A-Za-z0-9-_.]+$");
  await expect(page.getByTestId("section-details")).toContainText("larder-token-auth, the other authorizer here, caches for 300 seconds.");
  const call = page.getByTestId("call-panel");
  await expect(call).toContainText("CreateAuthorizer");
  await expect(call).toContainText('"type": "TOKEN"');
  await expect(call).toContainText('"identitySource": "method.request.header.Authorization"');
  await expect(call).toContainText("The type is TOKEN here; choosing Request makes it REQUEST.");
  await shot(page, "rest-authorizer-create");
  await page.getByTestId("form-submit").click();
  await expect(page.getByTestId("authorizer-banner")).toContainText("Authorizer partner-token-auth created.");
  await expect(page.getByTestId("authorizer-name").nth(1)).toBeVisible();
});

test("apigateway__rest-authorizers and rest-authorizers-none: what the authorizers read and call, then none left", async ({ page }) => {
  await go(page, apiPath("restapis", rest(), "?tab=authorizers"), "Authorizers");
  await expect(page.getByTestId("rest-page")).toContainText("larder-public has 2 authorizers, larder-token-auth and partner-token-auth.");
  const table = page.getByRole("table", { name: "Authorizers" });
  await expect(table).toContainText("Token");
  await expect(table).toContainText("token-auth");
  await expect(table).toContainText("Authorization");
  await expect(table).toContainText("300 seconds");
  await shot(page, "rest-authorizers");
  for (const name of ["larder-token-auth", "partner-token-auth"]) {
    await page.getByTestId(`delete-${name}`).click();
    await page.getByTestId("authorizer-delete-submit").click();
    await expect(page.getByTestId(`delete-${name}`)).toBeHidden();
  }
  await expect(page.getByRole("heading", { name: "No authorizers" })).toBeVisible();
  await expect(page.getByText("An authorizer checks a caller's identity before a method runs.")).toBeVisible();
  await shot(page, "rest-authorizers-none");
});
