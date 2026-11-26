import { describe, expect, test } from "bun:test";
import { deploymentBlocker, plansListing, resourceImpact, stagesServing, subtree } from "./delete-model";
import type { RestResource } from "./types";

const m = (httpMethod: string) => ({ httpMethod });
const tree: RestResource[] = [
  { id: "root", path: "/" },
  { id: "p", parentId: "root", pathPart: "products", path: "/products", resourceMethods: { GET: m("GET"), POST: m("POST") } },
  { id: "pid", parentId: "p", pathPart: "{id}", path: "/products/{id}", resourceMethods: { GET: m("GET") } },
  { id: "o", parentId: "root", pathPart: "orders", path: "/orders" },
];

describe("what deleting a resource takes with it", () => {
  test("is the resource and everything below it", () => {
    expect(subtree(tree, "p").map((r) => r.id)).toEqual(["p", "pid"]);
  });

  test("names the child resources and the methods of all of them", () => {
    const i = resourceImpact(tree, "p");
    expect(i?.children).toEqual(["/products/{id}"]);
    expect(i?.methods).toEqual(["GET /products", "POST /products", "GET /products/{id}"]);
    expect(i?.words).toBe("Its 1 child resource (/products/{id}) and all 3 methods (GET /products, POST /products and GET /products/{id}) go with it.");
  });

  test("says its 1 method, not all 1, when there is a child and one method", () => {
    const one: RestResource[] = [{ id: "root", path: "/" }, { id: "s", parentId: "root", pathPart: "s", path: "/s", resourceMethods: { GET: m("GET") } }, { id: "i", parentId: "s", pathPart: "i", path: "/s/i" }];
    expect(resourceImpact(one, "s")?.words).toBe("Its 1 child resource (/s/i) and its 1 method (GET /s) go with it.");
  });

  test("says so for a resource with nothing in it, and refuses the root", () => {
    expect(resourceImpact(tree, "o")?.words).toBe("It has no method and no child resource.");
    expect(resourceImpact(tree, "root")).toBeNull();
    expect(resourceImpact(tree, "nope")).toBeNull();
  });
});

describe("what stops a deployment being deleted", () => {
  test("a stage that serves it is named, in words", () => {
    const stages = [{ stageName: "prod", deploymentId: "d2" }, { stageName: "dev", deploymentId: "d1" }, { stageName: "qa", deploymentId: "d2" }];
    expect(stagesServing(stages, "d2")).toEqual(["prod", "qa"]);
    expect(deploymentBlocker(stagesServing(stages, "d2"))).toContain("prod and qa serve this deployment");
    expect(deploymentBlocker(stagesServing(stages, "d9"))).toBeNull();
  });
});

describe("the usage plans that list a stage", () => {
  test("are those naming this API and this stage", () => {
    const plans = [{ name: "gold", apiStages: [{ apiId: "a1", stage: "prod" }] }, { name: "free", apiStages: [{ apiId: "a2", stage: "prod" }] }, { name: "none" }];
    expect(plansListing(plans, "a1", "prod")).toEqual(["gold"]);
    expect(plansListing(plans, "a1", "dev")).toEqual([]);
  });
});

describe("what deleting a method takes with it", () => {
  test("is its integration and its responses", async () => {
    const { methodImpact } = await import("./delete-model");
    expect(methodImpact({ methodIntegration: { type: "MOCK", integrationResponses: { "200": { statusCode: "200" } } }, methodResponses: { "200": { statusCode: "200" }, "404": { statusCode: "404" } } })).toBe("Its integration with its responses and its 2 method responses go with it.");
    expect(methodImpact({ methodIntegration: { type: "MOCK" } })).toBe("Its integration goes with it.");
    expect(methodImpact({})).toBe("It has no integration and no method response.");
  });
});
