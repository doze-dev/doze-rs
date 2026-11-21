import { describe, expect, test } from "bun:test";
import { corsConfiguration, corsOf, corsProblem, defaultPath, emptyIntegration, emptyWizard, integrationInput, routeInput, routeKeyOf, routeProblem, sampleOf, wizardCalls, wizardProblem } from "./http-model";

const c = { region: "us-east-1", account: "111122223333" };

describe("an integration", () => {
  test("a Lambda function is AWS_PROXY with the payload version, an HTTP endpoint HTTP_PROXY", () => {
    expect(integrationInput("q8m", { ...emptyIntegration, fn: "export-report" }, c)).toEqual({ ApiId: "q8m", integrationType: "AWS_PROXY", integrationUri: "arn:aws:lambda:us-east-1:111122223333:function:export-report", payloadFormatVersion: "2.0" });
    expect(integrationInput("q8m", { ...emptyIntegration, kind: "http", url: "https://orders.larder.example.com/{proxy}" }, c)).toMatchObject({ integrationType: "HTTP_PROXY", integrationMethod: "ANY", payloadFormatVersion: "1.0" });
  });
});

describe("a route", () => {
  test("a key is the method and the path; a WebSocket key is bare", () => {
    expect(routeKeyOf("GET", "/reports")).toBe("GET /reports");
    expect(routeInput("q8m", "GET /x", "abc")).toEqual({ ApiId: "q8m", routeKey: "GET /x", target: "integrations/abc" });
    expect(routeProblem("GET", "reports", [], false)).toBe("A path starts with /, such as /reports.");
    expect(routeProblem("GET", "/reports", ["GET /reports"], false)).toBe("The API already has the route GET /reports.");
    expect(routeProblem("", "$default", [], true)).toBeNull();
  });
  test("a greedy route lands a sample request", () => {
    expect(sampleOf("ANY /orders/{proxy+}")).toBe("/orders/12");
  });
});

describe("the wizard", () => {
  test("four calls, the route named after the function, an automatic $default stage (the board's)", () => {
    const w = { ...emptyWizard, name: "larder-reports", integration: { ...emptyIntegration, fn: "export-report" } };
    const calls = wizardCalls(w, c);
    expect(calls.map((x) => x.operation)).toEqual(["CreateApi", "CreateIntegration", "CreateRoute", "CreateStage"]);
    expect(calls[0]!.input).toEqual({ name: "larder-reports", protocolType: "HTTP" });
    expect(calls[2]!.input).toEqual({ ApiId: "<API ID>", routeKey: "ANY /export-report", target: "integrations/<integration ID>" });
    expect(calls[3]!.input).toEqual({ ApiId: "<API ID>", stageName: "$default", autoDeploy: true });
    expect(defaultPath(w.integration)).toBe("/export-report");
    expect(wizardCalls(w, c, { api: "a1", integration: "i1" })[2]!.input).toMatchObject({ ApiId: "a1", target: "integrations/i1" });
  });
  test("each step says what is missing", () => {
    expect(wizardProblem(0, emptyWizard, [])).toBe("Enter an API name.");
    expect(wizardProblem(0, { ...emptyWizard, name: "a" }, [])).toBe("Choose the Lambda function.");
    expect(wizardProblem(2, { ...emptyWizard, stage: "a b" }, [])).toContain("A stage name");
  });
});

describe("CORS", () => {
  test("the configuration leaves out the lists the form leaves empty, and says allowCredentials", () => {
    const f = { ...corsOf(undefined), origins: ["https://admin.larder.example.com"], methods: ["GET", "POST"], maxAge: "300" };
    expect(corsConfiguration(f)).toEqual({ allowOrigins: ["https://admin.larder.example.com"], allowMethods: ["GET", "POST"], maxAge: 300, allowCredentials: false });
  });
  test("reads what the API has, and refuses what AWS refuses", () => {
    expect(corsOf({ corsConfiguration: { allowOrigins: ["*"], maxAge: 5 } })).toMatchObject({ origins: ["*"], maxAge: "5" });
    expect(corsProblem({ ...corsOf(undefined), maxAge: "90000" })).toBe("Max age is -1 to 86,400 seconds.");
    expect(corsProblem({ ...corsOf(undefined), origins: ["*"], credentials: true })).toContain("Browsers refuse a *");
  });
});
