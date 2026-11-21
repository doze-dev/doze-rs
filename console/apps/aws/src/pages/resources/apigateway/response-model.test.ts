import { describe, expect, test } from "bun:test";
import { integrationResponseInput, integrationResponseProblem, methodResponseInput, methodResponseProblem } from "./response-model";

const base = { api: "k3x", resource: "r8w", method: "GET" };

describe("a method response", () => {
  test("a status, its headers and its model by content type", () => {
    expect(methodResponseInput(base, { status: "404", headers: "X-Id, X-Trace", contentType: "application/json", model: "Error" })).toEqual({
      restApiId: "k3x", resourceId: "r8w", httpMethod: "GET", statusCode: "404",
      responseParameters: { "method.response.header.X-Id": false, "method.response.header.X-Trace": false },
      responseModels: { "application/json": "Error" },
    });
    expect(methodResponseProblem({ status: "200", headers: "", contentType: "", model: "" }, ["200"])).toContain("already has a 200 response");
    expect(methodResponseProblem({ status: "20", headers: "", contentType: "", model: "" }, [])).toContain("three digits");
  });
});

describe("an integration response", () => {
  test("the regex that picks it, its header mappings and its template", () => {
    const f = { status: "400", pattern: "4\\d\\d", headers: [{ name: "X-Why", value: "'bad'" }, { name: "", value: "" }], template: "{\"error\": 1}", contentType: "application/json" };
    expect(integrationResponseInput(base, f)).toEqual({ restApiId: "k3x", resourceId: "r8w", httpMethod: "GET", statusCode: "400", selectionPattern: "4\\d\\d", responseParameters: { "method.response.header.X-Why": "'bad'" }, responseTemplates: { "application/json": "{\"error\": 1}" } });
    expect(integrationResponseProblem({ ...f, pattern: "(" }, [])).toContain("not a regular expression");
    expect(integrationResponseProblem({ ...f, headers: [{ name: "X", value: "" }] }, [])).toBe("A header mapping needs a name and a value.");
  });
});
