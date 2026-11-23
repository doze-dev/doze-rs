import { describe, expect, test } from "bun:test";
import { escapePointer, editProblem, fromIntegration, integrationPatches, shapes } from "./integration-model";
import type { Integration } from "./types";

const lambda: Integration = { type: "AWS", httpMethod: "POST", uri: "arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:1:function:f/invocations", requestTemplates: { "application/json": '{"id": "$input.params(\'id\')"}' }, requestParameters: { "integration.request.querystring.page": "method.request.querystring.p" }, passthroughBehavior: "WHEN_NO_MATCH" };

describe("an integration request, edited", () => {
  test("the form starts as the integration is, and saving it unchanged patches nothing", () => {
    const e = fromIntegration(lambda);
    expect(e).toMatchObject({ defaultTimeout: true, timeout: "29000", passthrough: "WHEN_NO_MATCH" });
    expect(e.templates.map((t) => t.type)).toEqual(["application/json"]);
    expect(e.params).toMatchObject([{ where: "querystring", name: "page", from: "method.request.querystring.p" }]);
    expect(integrationPatches(lambda, e)).toEqual([]);
  });
  test("a timeout, a template and a parameter are replaced, added and removed one by one", () => {
    const e = fromIntegration(lambda);
    const edited = {
      ...e,
      defaultTimeout: false,
      timeout: "5000",
      passthrough: "NEVER" as const,
      templates: [{ ...e.templates[0]!, text: "{}" }, { id: 90, type: "text/plain", text: "hello" }],
      params: [{ id: 91, where: "header" as const, name: "X-Id", from: "method.request.header.id" }],
    };
    expect(integrationPatches(lambda, edited)).toEqual([
      { op: "replace", path: "/timeoutInMillis", value: "5000" },
      { op: "replace", path: "/passthroughBehavior", value: "NEVER" },
      { op: "replace", path: "/requestTemplates/application~1json", value: "{}" },
      { op: "add", path: "/requestTemplates/text~1plain", value: "hello" },
      { op: "add", path: "/requestParameters/integration.request.header.X-Id", value: "method.request.header.id" },
      { op: "remove", path: "/requestParameters/integration.request.querystring.page" },
    ]);
  });
  test("a proxy integration has its timeout edited, nothing else", () => {
    const proxy: Integration = { type: "AWS_PROXY", requestTemplates: { "application/json": "x" } };
    expect(shapes(proxy)).toBe(false);
    const e = { ...fromIntegration(proxy), defaultTimeout: false, timeout: "100", templates: [] };
    expect(integrationPatches(proxy, e)).toEqual([{ op: "replace", path: "/timeoutInMillis", value: "100" }]);
  });
  test("what is wrong", () => {
    const e = fromIntegration(lambda);
    expect(editProblem(e, false)).toBeNull();
    expect(editProblem({ ...e, defaultTimeout: false, timeout: "49" }, false)).toBe("A custom timeout is 50 to 29,000 milliseconds.");
    expect(editProblem({ ...e, defaultTimeout: false, timeout: "29001" }, true)).toContain("29,000");
    expect(editProblem({ ...e, templates: [{ id: 1, type: " ", text: "" }] }, false)).toContain("content type");
    expect(editProblem({ ...e, templates: [{ id: 1, type: "a/b", text: "" }, { id: 2, type: "a/b", text: "" }] }, false)).toBe("Each content type has one template.");
    expect(editProblem({ ...e, params: [{ id: 1, where: "path", name: "", from: "x" }] }, false)).toContain("name");
    expect(editProblem({ ...e, params: [{ id: 1, where: "path", name: "a", from: "x" }, { id: 2, where: "path", name: "a", from: "y" }] }, false)).toBe("A parameter is mapped once.");
    // A proxy's templates and parameters are not the form's.
    expect(editProblem({ ...e, templates: [{ id: 1, type: "", text: "" }] }, true)).toBeNull();
  });
  test("a pointer's slash is written ~1", () => {
    expect(escapePointer("application/json")).toBe("application~1json");
    expect(escapePointer("a~b")).toBe("a~0b");
  });
});
