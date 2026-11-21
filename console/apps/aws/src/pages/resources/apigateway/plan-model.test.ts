import { describe, expect, test } from "bun:test";
import { addStagePatch, emptyPlan, keyProblem, planInput, planProblem, removeStagePatch } from "./plan-model";

describe("a usage plan's form", () => {
  test("throttle and quota as the board's call shows them", () => {
    expect(planInput({ ...emptyPlan, name: "partners-premium", description: "Premium", rate: "200", burst: "400", limit: "100000" })).toEqual({ name: "partners-premium", description: "Premium", throttle: { rateLimit: 200, burstLimit: 400 }, quota: { limit: 100000, period: "MONTH" } });
    expect(planInput({ ...emptyPlan, name: "bare" })).toEqual({ name: "bare" });
  });
  test("what is wrong", () => {
    expect(planProblem(emptyPlan)).toBe("Enter a name for the plan.");
    expect(planProblem({ ...emptyPlan, name: "a", rate: "5" })).toBe("Throttling needs both a rate and a burst.");
    expect(planProblem({ ...emptyPlan, name: "a", rate: "x", burst: "1" })).toContain("number of requests");
    expect(planProblem({ ...emptyPlan, name: "a", limit: "1.5" })).toContain("quota");
    expect(planProblem({ ...emptyPlan, name: "a", rate: "5", burst: "10", limit: "100" })).toBeNull();
  });
  test("a stage is added and taken off by a patch of /apiStages", () => {
    expect(addStagePatch("k3x", "prod")).toEqual([{ op: "add", path: "/apiStages", value: "k3x:prod" }]);
    expect(removeStagePatch("k3x", "prod")[0]!.op).toBe("remove");
    expect(keyProblem(" ")).toBe("Enter a name for the key.");
  });
});
