import { describe, expect, test } from "bun:test";
import { corsCalls, createResourceInput, fullPath, partOf, resourceProblem } from "./resource-model";

describe("a resource's form", () => {
  test("a proxy resource is {proxy+}, whatever was typed", () => {
    expect(partOf({ proxy: true, parent: "r", name: "x", cors: false })).toBe("{proxy+}");
    expect(createResourceInput("k3x", { proxy: false, parent: "r8w2k5", name: "{sku}", cors: false })).toEqual({ restApiId: "k3x", parentId: "r8w2k5", pathPart: "{sku}" });
  });
  test("the full path and what is wrong", () => {
    expect(fullPath("/", "orders")).toBe("/orders");
    expect(fullPath("/products", "{sku}")).toBe("/products/{sku}");
    expect(resourceProblem({ proxy: false, parent: "r", name: "", cors: false })).toBe("Enter a resource name.");
    expect(resourceProblem({ proxy: false, parent: "", name: "a", cors: false })).toBe("Choose where the resource goes.");
  });
  test("CORS is the OPTIONS method: a mock and its two responses", () => {
    const calls = corsCalls("k3x", "r8w");
    expect(calls.map((c) => c.operation)).toEqual(["PutMethod", "PutIntegration", "PutMethodResponse", "PutIntegrationResponse"]);
    expect(calls[1]!.input).toMatchObject({ type: "MOCK", requestHttpMethod: "OPTIONS" });
  });
});
