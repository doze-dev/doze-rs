import { describe, expect, test } from "bun:test";
import { deletedRecord, failureOf, failureSentence, stackSentence } from "./sentence";

describe("a stack's sentence", () => {
  test("a stack whose create failed holds nothing", () => {
    const s = stackSentence("cfa-search", "ROLLBACK_COMPLETE", "", "Nov 1", null);
    expect(s).toContain("holds nothing");
    expect(s).not.toContain("held before");
  });
  test("a stack whose update failed holds what it held", () => {
    expect(stackSentence("x", "UPDATE_ROLLBACK_COMPLETE", "", null, null)).toContain("holds what it held before the update");
  });
  test("a stack under review waits for its change set", () => {
    const s = stackSentence("cfa-review", "REVIEW_IN_PROGRESS", "", null, null);
    expect(s).toContain("waiting for you to execute its change set");
    expect(s).not.toContain("being worked on");
  });
  test("a stack at work is being worked on, and a failed one says why", () => {
    expect(stackSentence("x", "CREATE_IN_PROGRESS", "", null, null)).toContain("being worked on");
    expect(stackSentence("x", "CREATE_FAILED", "A failed", null, null)).toContain("Reason: A failed");
  });
});

describe("what failed", () => {
  const events = [
    { LogicalResourceId: "s", ResourceType: "AWS::CloudFormation::Stack", ResourceStatus: "ROLLBACK_COMPLETE" },
    { LogicalResourceId: "B", ResourceType: "AWS::SSM::Parameter", ResourceStatus: "CREATE_FAILED", ResourceStatusReason: "later" },
    { LogicalResourceId: "A", ResourceType: "AWS::SSM::Parameter", ResourceStatus: "CREATE_FAILED", ResourceStatusReason: "already exists" },
    { LogicalResourceId: "s", ResourceType: "AWS::CloudFormation::Stack", ResourceStatus: "CREATE_IN_PROGRESS" },
  ];
  test("the first resource to fail is the cause", () => {
    expect(failureOf(events)).toEqual({ resource: "A", type: "AWS::SSM::Parameter", reason: "already exists" });
    expect(failureSentence(failureOf(events)!)).toBe("A (AWS::SSM::Parameter) failed to create: already exists");
  });
  test("no failed resource, no cause", () => {
    expect(failureOf([])).toBeNull();
    expect(failureOf([{ LogicalResourceId: "s", ResourceType: "AWS::CloudFormation::Stack", ResourceStatus: "CREATE_FAILED" }])).toBeNull();
  });
});

describe("a deleted stack", () => {
  test("is found by name, the newest record", () => {
    const list = [
      { StackName: "x", StackId: "old", DeletionTime: 100 },
      { StackName: "y", StackId: "other", DeletionTime: 300 },
      { StackName: "x", StackId: "new", DeletionTime: 200 },
    ];
    expect(deletedRecord(list, "x")).toEqual({ id: "new", deleted: "200" });
    expect(deletedRecord(list, "z")).toBeNull();
  });
});
