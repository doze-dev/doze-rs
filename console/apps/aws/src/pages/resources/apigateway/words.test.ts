import { describe, expect, test } from "bun:test";
import type { HttpApi, RestApi, RestResource } from "./types";
import { anyApis, flattenTree, httpStagesSentence, httpTarget, integrationKind, invokeUrls, lambdaName, methodsOf, pathPartProblem, quotaWords, restSentence, routesSentence, stageNameProblem, throttleWords, variableProblem } from "./words";

const rest: RestApi = { id: "k3x", name: "larder-public", createdDate: 1_000, endpointConfiguration: { types: ["REGIONAL"] } };
const http: HttpApi = { apiId: "q8m", name: "larder-admin", protocolType: "HTTP", createdDate: "1970-01-01T00:33:20Z" };
const tree: RestResource[] = [
  { id: "r", path: "/" },
  { id: "b", parentId: "r", pathPart: "receipts", path: "/receipts", resourceMethods: { POST: { httpMethod: "POST" } } },
  { id: "a", parentId: "r", pathPart: "products", path: "/products", resourceMethods: { OPTIONS: { httpMethod: "OPTIONS" }, GET: { httpMethod: "GET" } } },
  { id: "c", parentId: "a", pathPart: "{sku}", path: "/products/{sku}" },
];

describe("the one list of APIs", () => {
  test("both kinds in the order they were made, words for the endpoint type", () => {
    const all = anyApis([rest], [http]);
    expect(all.map((a) => [a.name, a.protocol, a.endpointType])).toEqual([
      ["larder-public", "REST", "Regional"],
      ["larder-admin", "HTTP", "Regional"],
    ]);
  });
});

describe("a resource tree", () => {
  test("is read parent first, children by path, with the depth", () => {
    expect(flattenTree(tree).map((n) => `${n.depth}${n.resource.path}`)).toEqual(["0/", "1/products", "2/products/{sku}", "1/receipts"]);
  });
  test("a resource's methods are read GET first and OPTIONS last", () => {
    expect(methodsOf(tree[2]!).map((m) => m.httpMethod)).toEqual(["GET", "OPTIONS"]);
  });
});

describe("what an integration is", () => {
  test("a function by its invocation URI or its ARN, a URL, a mock", () => {
    const uri = "arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:111122223333:function:export-report/invocations";
    expect(lambdaName(uri)).toBe("export-report");
    expect(integrationKind({ type: "AWS_PROXY", uri })).toBe("Lambda function");
    expect(integrationKind({ type: "HTTP", uri: "https://x" })).toBe("HTTP");
    expect(integrationKind({ type: "MOCK" })).toBe("Mock");
    expect(integrationKind(undefined)).toBe("None");
    expect(httpTarget("AWS_PROXY", "arn:aws:lambda:us-east-1:1:function:export-report")).toBe("export-report Lambda");
  });
});

describe("the sentences", () => {
  test("a REST API says what it has and where it is deployed", () => {
    const s = restSentence("larder-public", tree, [{ stageName: "prod", deploymentId: "d4h" }], [{ id: "d4h", createdDate: 1_000 }]);
    expect(s).toBe("larder-public has 4 resources and 3 methods, and one stage: prod, pointing at deployment d4h from 1970-01-01 00:16:40 UTC.");
    expect(restSentence("x", [], [], [])).toBe("x has 0 resources and 0 methods, and has never been deployed.");
  });
  test("an HTTP API's routes say which ones have no integration", () => {
    const routes = [{ routeKey: "GET /reports", target: "integrations/a" }, { routeKey: "POST /reports/run", target: "integrations/a" }, { routeKey: "ANY /orders/{proxy+}" }];
    const info = { fn: (t: string | undefined) => (t ? "export-report" : null), authorizer: (id: string | undefined) => (id ? "the Lambda authorizer admin-request-auth" : null) };
    expect(routesSentence("larder-admin", routes.map((r) => ({ ...r, authorizerId: r.target ? "a1" : undefined })), [{ stageName: "$default" }], info)).toBe(
      "larder-admin has 3 routes on one stage, $default. Two routes run the Lambda function export-report behind the Lambda authorizer admin-request-auth; ANY /orders/{proxy+} has no integration yet.",
    );
    expect(routesSentence("x", [{ routeKey: "GET /a", target: "i" }], [], { ...info, authorizer: () => null })).toBe("x has 1 route on no stage. One route runs the Lambda function export-report.");
  });
  test("an HTTP API's stages say what is automatic", () => {
    const s = httpStagesSentence("larder-admin", [{ stageName: "$default", autoDeploy: true, deploymentId: "f6k" }, { stageName: "beta", deploymentId: "b7w" }], [{ deploymentId: "f6k", createdDate: "1970-01-01T00:16:40Z" }]);
    expect(s).toContain("$default releases every change by itself; its last deployment, f6k, went out 1970-01-01 00:16:40 UTC.");
    expect(s).toContain("beta is deployed by hand and points at b7w.");
    expect(httpStagesSentence("larder-hooks", [], [])).toBe("larder-hooks has no stage, so it cannot be invoked.");
  });
});

describe("the addresses of a stage", () => {
  test("the $default stage of an HTTP API has no stage in its path, any other has", () => {
    const o = { id: "q8m", region: "us-east-1", endpoint: "http://127.0.0.1:4566", http: true };
    expect(invokeUrls({ ...o, stage: "$default" })).toEqual({ aws: "https://q8m.execute-api.us-east-1.amazonaws.com", local: "http://127.0.0.1:4566/_aws/execute-api/q8m" });
    expect(invokeUrls({ ...o, stage: "beta" }).aws).toBe("https://q8m.execute-api.us-east-1.amazonaws.com/beta");
    expect(invokeUrls({ ...o, http: false, stage: "prod" }).local).toBe("http://127.0.0.1:4566/_aws/execute-api/q8m/prod");
    // A WebSocket API is wss:// (ws:// here), and its $default stage is in the path like any other.
    expect(invokeUrls({ ...o, websocket: true, stage: "$default" })).toEqual({ aws: "wss://q8m.execute-api.us-east-1.amazonaws.com/$default", local: "ws://127.0.0.1:4566/_aws/execute-api/q8m/$default" });
    expect(invokeUrls({ ...o, websocket: true, stage: "prod" }).aws).toBe("wss://q8m.execute-api.us-east-1.amazonaws.com/prod");
  });
});

describe("a usage plan", () => {
  test("throttle and quota in words", () => {
    expect(throttleWords({ rateLimit: 50, burstLimit: 100 })).toBe("50 per second, burst 100");
    expect(quotaWords({ limit: 10000, period: "MONTH" })).toBe("10,000 per month");
    expect(throttleWords(undefined)).toBe("Not set");
  });
});

describe("what a name may be", () => {
  test("stages, path parts and variables", () => {
    expect(stageNameProblem("prod")).toBeNull();
    expect(stageNameProblem("a b")).not.toBeNull();
    expect(pathPartProblem("{sku}")).toBeNull();
    expect(pathPartProblem("{proxy+}")).toBeNull();
    expect(pathPartProblem("a/b")).not.toBeNull();
    expect(variableProblem("thumbSize", "256")).toBeNull();
    expect(variableProblem("bad-name", "x")).not.toBeNull();
  });
});

import { apisSentence } from "./words";
describe("the sentence over the list of APIs", () => {
  test("says which stage each answers at and how it is deployed", () => {
    const apis = anyApis([rest], [http]);
    const s = apisSentence(apis, { k3x: [{ stage: "prod", auto: false, deployed: "1970-01-01 00:16:40 UTC" }], q8m: [{ stage: "$default", auto: true, deployed: null }] });
    expect(s).toBe("larder-public answers through its prod stage, deployed 1970-01-01 00:16:40 UTC. larder-admin answers at its $default stage, which deploys every change you save.");
    expect(apisSentence(apis, { k3x: [], q8m: [] })).toBe("larder-public has no stage yet, so it cannot be invoked. larder-admin has no stage yet, so it cannot be invoked.");
    // The rest past the first `most` are counted, and the verb agrees with the count.
    const stage = { stage: "prod", auto: false, deployed: null };
    expect(apisSentence(apis, { k3x: [stage], q8m: [stage] }, 1)).toEndWith("1 more API answers too.");
    const three = anyApis([rest, { ...rest, id: "a", name: "a" }, { ...rest, id: "b", name: "b" }], []);
    expect(apisSentence(three, { k3x: [stage], a: [stage], b: [stage] }, 1)).toEndWith("2 more APIs answer too.");
  });
});

import { stageSentence } from "./words";
describe("what a stage points at", () => {
  test("the deployment it has and the one before it", () => {
    const s = stageSentence({ stageName: "prod", deploymentId: "m2c" }, [{ id: "d4h", createdDate: 1_000 }, { id: "m2c", createdDate: 2_000, description: "Add POST /products" }]);
    expect(s).toBe("Stage prod points at deployment m2c, created 1970-01-01 00:33:20 UTC with the description “Add POST /products”. The one before it, d4h from 1970-01-01 00:16:40 UTC, is kept in the deployment history.");
    expect(stageSentence({ stageName: "x" }, [])).toBe("Stage x points at no deployment yet.");
  });
});

import { backendName, methodSentence } from "./words";
describe("a method in words", () => {
  const get = { httpMethod: "GET", authorizationType: "NONE", requestParameters: { "method.request.querystring.category": false, "method.request.querystring.page": false }, methodIntegration: { type: "HTTP", uri: "https://catalog.larder.example.com/v1/products" } };
  test("the backend by host, function, mock or action", () => {
    expect(backendName(get.methodIntegration)).toBe("catalog.larder.example.com");
    expect(backendName({ type: "MOCK" })).toBe("Mock");
    expect(backendName({ type: "AWS", uri: "arn:aws:apigateway:us-east-1:sqs:action/SendMessage" })).toBe("sqs SendMessage");
  });
  test("the sentence over a method", () => {
    expect(methodSentence(get, "/products", undefined)).toBe("A GET to /products needs no authorization and no API key. It accepts two query strings, category and page, and the catalog.larder.example.com backend answers.");
    expect(methodSentence({ httpMethod: "POST", authorizationType: "CUSTOM", apiKeyRequired: true, methodIntegration: { type: "MOCK" } }, "/x", "tok")).toBe("A POST to /x is checked by the authorizer tok and an API key. It accepts no query string, and API Gateway answers it itself.");
  });
});

import { haveApis } from "./words";
test("what the Region already has", () => {
  expect(haveApis(["larder-public"], ["larder-admin"])).toBe("a REST API, larder-public, and an HTTP API, larder-admin");
  expect(haveApis(["a", "b"], [])).toBe("2 REST APIs, a and b");
});

import { methodPhrase } from "./words";
test("a resource's methods in words", () => {
  expect(methodPhrase(["GET", "OPTIONS"])).toBe("a GET and an OPTIONS method");
  expect(methodPhrase([])).toBe("no method");
});
