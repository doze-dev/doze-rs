import { describe, expect, test } from "bun:test";
import { addStagePatch, emptyPlan, firstWithFreeStage, keyProblem, planInput, planProblem, removeStagePatch } from "./plan-model";

describe("a usage plan's form", () => {
  test("throttle and quota as the board's call shows them", () => {
    expect(planInput({ ...emptyPlan, name: "partners-premium", description: "Premium", throttle: true, rate: "200", burst: "400", quota: true, limit: "100000" })).toEqual({ name: "partners-premium", description: "Premium", throttle: { rateLimit: 200, burstLimit: 400 }, quota: { limit: 100000, period: "MONTH" } });
    expect(planInput({ ...emptyPlan, name: "bare" })).toEqual({ name: "bare" });
    // A toggle that is off leaves its limit out, whatever waits in its fields; the other still goes.
    expect(planInput({ ...emptyPlan, name: "q", quota: true })).toEqual({ name: "q", quota: { limit: 10000, period: "MONTH" } });
    expect(planInput({ ...emptyPlan, name: "t", throttle: true })).toEqual({ name: "t", throttle: { rateLimit: 100, burstLimit: 200 } });
  });
  test("what is wrong", () => {
    expect(planProblem(emptyPlan)).toBe("Enter a name for the plan.");
    expect(planProblem({ ...emptyPlan, name: "a", throttle: true, burst: "" })).toBe("Throttling needs both a rate and a burst.");
    expect(planProblem({ ...emptyPlan, name: "a", throttle: true, rate: "x", burst: "1" })).toContain("number of requests");
    expect(planProblem({ ...emptyPlan, name: "a", quota: true, limit: "1.5" })).toContain("quota");
    expect(planProblem({ ...emptyPlan, name: "a", throttle: true, rate: "5", burst: "10", quota: true, limit: "100" })).toBeNull();
    // What waits in the fields of a toggle that is off is not a problem.
    expect(planProblem({ ...emptyPlan, name: "a", rate: "x", limit: "1.5" })).toBeNull();
  });
  test("the API offered for a stage is one that has a stage the plan does not cover", () => {
    const apis = [{ id: "a" }, { id: "b" }, { id: "c" }];
    const stages = { a: [], b: [{ stageName: "prod" }], c: [{ stageName: "prod" }, { stageName: "beta" }] };
    expect(firstWithFreeStage(apis, stages, [])).toBe("b");
    expect(firstWithFreeStage(apis, stages, [{ apiId: "b", stage: "prod" }])).toBe("c");
    expect(firstWithFreeStage(apis, stages, [{ apiId: "b", stage: "prod" }, { apiId: "c", stage: "prod" }, { apiId: "c", stage: "beta" }])).toBe("a");
    expect(firstWithFreeStage(apis, {}, [])).toBe("a");
    expect(firstWithFreeStage([], {}, [])).toBe("");
  });
  test("a stage is added and taken off by a patch of /apiStages", () => {
    expect(addStagePatch("k3x", "prod")).toEqual([{ op: "add", path: "/apiStages", value: "k3x:prod" }]);
    expect(removeStagePatch("k3x", "prod")[0]!.op).toBe("remove");
    expect(keyProblem(" ")).toBe("Enter a name for the key.");
  });
});
