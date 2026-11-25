import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import { ageSeconds, emptyRule, firstProblem, nameProblem, newTarget, patternProblem, putRuleInput, putTargetsInput, targetId } from "./rule-model";

const A = "arn:aws:sns:us-east-1:111122223333:orders-created";

describe("a rule's fields", () => {
  test("its name", () => {
    expect(nameProblem("order-placed")).toBeNull();
    expect(nameProblem("")).toBe("Name the rule.");
    expect(nameProblem("a b")).toContain("letters, digits");
    expect(nameProblem("x".repeat(65))).toContain("64");
  });
  test("its pattern", () => {
    expect(patternProblem('{"source":["a"]}')).toBeNull();
    expect(patternProblem("[]")).toBe("An event pattern is a JSON object.");
    expect(patternProblem("{}")).toContain("empty pattern");
    expect(patternProblem("{")).toContain("Not JSON");
    // The shape PutRule refuses at the end is refused at Next: a key holds an array or an object.
    expect(patternProblem('{"source":"x"}')).toBe('"source" must be an object or an array.');
    expect(patternProblem('{"detail":{"status":"ok"}}')).toBe('"status" must be an object or an array.');
    expect(patternProblem('{"source":[]}')).toBe("Empty arrays are not allowed.");
    expect(patternProblem('{"$or":[{"source":["a"]},{"detail-type":["b"]}]}')).toBeNull();
    expect(patternProblem('{"$or":["a"]}')).toBe("$or must contain only objects.");
    expect(patternProblem('{"detail":{"n":[{"numeric":[">",5]}]}}')).toBeNull();
    expect(patternProblem(JSON.stringify({ a: ["x".repeat(2100)] }))).toContain("2,048");
  });
  test("a retry policy's age", () => {
    expect(ageSeconds({ hours: "1", minutes: "0", seconds: "0" })).toEqual({ seconds: 3600 });
    expect(ageSeconds({ hours: "0", minutes: "0", seconds: "30" })).toEqual({ problem: "The maximum age is 60 seconds to 24 hours." });
    expect(ageSeconds({ hours: "24", minutes: "1", seconds: "0" })).toEqual({ problem: "The maximum age is 60 seconds to 24 hours." });
  });
});

describe("what the wizard sends", () => {
  const d = { ...emptyRule("pattern", "larder"), name: "order-placed", description: "Fan out", tags: [newRow("Team", "checkout"), newRow()] };
  d.targets = [{ ...newTarget("sns"), arn: A, input: "transformer" as const, paths: '{"order_id":"$.detail.order_id"}', template: '{"o":"<order_id>"}', retry: true, hours: "1", attempts: "20", dlq: "arn:aws:sqs:us-east-1:111122223333:dlq" }];
  test("PutRule with its tags, blank rows left out", () => {
    expect(putRuleInput(d)).toEqual({ Name: "order-placed", EventBusName: "larder", Description: "Fan out", EventPattern: d.pattern, State: "ENABLED", Tags: [{ Key: "Team", Value: "checkout" }] });
  });
  test("PutTargets with the transformer, the retry policy and the dead-letter queue", () => {
    expect(putTargetsInput(d)).toEqual({ Rule: "order-placed", EventBusName: "larder", Targets: [{ Id: "publish-orders-created", Arn: A, InputTransformer: { InputPathsMap: { order_id: "$.detail.order_id" }, InputTemplate: '{"o":"<order_id>"}' }, RetryPolicy: { MaximumRetryAttempts: 20, MaximumEventAgeInSeconds: 3600 }, DeadLetterConfig: { Arn: "arn:aws:sqs:us-east-1:111122223333:dlq" } }] });
  });
  test("a schedule rule sends the schedule", () => {
    expect(putRuleInput({ ...emptyRule("schedule"), name: "tick", enabled: false })).toEqual({ Name: "tick", EventBusName: "default", ScheduleExpression: "cron(0 18 ? * MON-FRI *)", State: "DISABLED" });
  });
  test("target ids are unique", () => {
    expect(targetId({ kind: "sqs", arn: "arn:aws:sqs:us-east-1:1:q" }, ["send-q"])).toBe("send-q-2");
  });
});

describe("the first step that cannot be passed", () => {
  test("names, patterns, targets", () => {
    const d = emptyRule("pattern", "larder");
    expect(firstProblem(d, [])).toEqual({ step: 0, why: "Name the rule." });
    expect(firstProblem({ ...d, name: "a" }, ["a"])).toEqual({ step: 0, why: "a is already on larder." });
    expect(firstProblem({ ...d, name: "a", pattern: "{" }, [])?.step).toBe(1);
    expect(firstProblem({ ...d, name: "a" }, [])).toEqual({ step: 2, why: "Choose where the target sends." });
    expect(firstProblem({ ...d, name: "a", targets: [{ ...d.targets[0]!, arn: A }] }, [])).toBeNull();
  });
});
