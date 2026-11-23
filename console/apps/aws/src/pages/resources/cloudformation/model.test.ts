import { describe, expect, test } from "bun:test";
import { deleteRefusal, deletedRecord, deletionSplit, failedResource, isGone, listSummary, nameList, noStack, resourceTarget, retainedIn, serviceCount, stackRow, stackTone } from "./model";

const row = (name: string, status: string, parent = "") => stackRow({ name, cells: [status, null, "2026-11-07", null, parent || null] });

describe("status and errors", () => {
  test("a status has a colour", () => {
    expect(stackTone("CREATE_COMPLETE")).toBe("ok");
    expect(stackTone("ROLLBACK_COMPLETE")).toBe("bad");
    expect(stackTone("UPDATE_ROLLBACK_COMPLETE")).toBe("warn");
    expect(stackTone("REVIEW_IN_PROGRESS")).toBe("info");
    expect(stackTone("UPDATE_FAILED")).toBe("bad");
    expect(stackTone("DELETE_COMPLETE")).toBe("idle");
  });
  test("a stack that is not there is told from another refusal", () => {
    expect(isGone({ status: 400, code: "ValidationError", message: "Stack with id x does not exist" })).toBe(true);
    expect(isGone({ status: 400, code: "ValidationError", message: "Template format error" })).toBe(false);
    expect(isGone({ status: 500, code: "ValidationError", message: "does not exist" })).toBe(false);
  });
  test("the console reads a stack that is gone as an empty answer", () => {
    expect(noStack({ Stacks: [] })).toBe(true);
    expect(noStack({})).toBe(true);
    expect(noStack(undefined)).toBe(false);
    expect(noStack({ Stacks: [{}] })).toBe(false);
  });
});

describe("the list's sentence", () => {
  test("counts the stacks and names the ones that need a look", () => {
    const s = listSummary([row("larder-edge", "UPDATE_COMPLETE"), row("larder-edge-ApiStack", "UPDATE_COMPLETE", "arn:parent"), row("larder-reports-job", "REVIEW_IN_PROGRESS"), row("larder-hotfix", "UPDATE_ROLLBACK_COMPLETE"), row("larder-nightly", "ROLLBACK_COMPLETE")], "us-east-1");
    expect(s.text).toBe("5 stacks in us-east-1, one of them nested. larder-reports-job is waiting for its change set to be executed; larder-hotfix and larder-nightly rolled back.");
    expect(s.names).toEqual(["larder-reports-job", "larder-hotfix", "larder-nightly"]);
  });
  test("says only the count when nothing needs a look, and singular for one", () => {
    expect(listSummary([row("a", "CREATE_COMPLETE")], "us-east-1").text).toBe("1 stack in us-east-1.");
    expect(listSummary([row("a", "CREATE_FAILED")], "r").text).toBe("1 stack in r. a failed.");
    expect(listSummary([row("a", "UPDATE_IN_PROGRESS"), row("b", "DELETE_IN_PROGRESS")], "r").text).toContain("a and b are being worked on");
  });
  test("names as a list", () => {
    expect(nameList(["a"])).toBe("a");
    expect(nameList(["a", "b", "c"])).toBe("a, b and c");
  });
});

describe("what a stack owns", () => {
  test("a resource has a page of its own when the console has one", () => {
    expect(resourceTarget({ ResourceType: "AWS::SQS::Queue", PhysicalResourceId: "http://127.0.0.1:1/111122223333/pick-lists" })).toEqual({ service: "sqs", kind: "queues", name: "pick-lists" });
    expect(resourceTarget({ ResourceType: "AWS::SNS::Topic", PhysicalResourceId: "arn:aws:sns:us-east-1:1:orders" })).toEqual({ service: "sns", kind: "topics", name: "orders" });
    expect(resourceTarget({ ResourceType: "AWS::S3::Bucket", PhysicalResourceId: "product-images" })?.kind).toBe("buckets");
    expect(resourceTarget({ ResourceType: "AWS::EC2::Instance", PhysicalResourceId: "i-1" })).toBeNull();
    expect(resourceTarget({ ResourceType: "AWS::S3::Bucket" })).toBeNull();
  });
  test("counts the services", () => {
    const r = (t: string) => ({ LogicalResourceId: t, ResourceType: t, ResourceStatus: "CREATE_COMPLETE" });
    expect(serviceCount([r("AWS::SQS::Queue"), r("AWS::SQS::Queue"), r("AWS::SNS::Topic")])).toBe(2);
  });
});

describe("what the template retains", () => {
  test("a JSON template", () => {
    const t = JSON.stringify({ Resources: { Data: { Type: "AWS::S3::Bucket", DeletionPolicy: "Retain" }, Q: { Type: "AWS::SQS::Queue" }, L: { Type: "AWS::Logs::LogGroup", DeletionPolicy: "Delete" }, K: { Type: "AWS::KMS::Key", DeletionPolicy: "RetainExceptOnCreate" } } });
    expect(retainedIn(t)).toEqual([{ id: "Data", type: "AWS::S3::Bucket" }, { id: "K", type: "AWS::KMS::Key" }]);
  });
  test("a YAML template, read line by line", () => {
    const y = "AWSTemplateFormatVersion: '2010-09-09'\nResources:\n  Data:\n    Type: AWS::S3::Bucket\n    DeletionPolicy: Retain\n  Queue:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: q\n  Table:\n    DeletionPolicy: \"Retain\" # keep it\n    Type: AWS::DynamoDB::Table\nOutputs:\n  X:\n    Value: 1\n";
    expect(retainedIn(y)).toEqual([{ id: "Data", type: "AWS::S3::Bucket" }, { id: "Table", type: "AWS::DynamoDB::Table" }]);
    expect(retainedIn("")).toEqual([]);
  });
});

describe("the delete", () => {
  const r = (id: string, status = "CREATE_COMPLETE") => ({ LogicalResourceId: id, ResourceType: "AWS::SQS::Queue", ResourceStatus: status });
  test("splits what goes from what stays", () => {
    const { deleted, kept } = deletionSplit([r("A"), r("B"), r("C", "DELETE_COMPLETE")], [{ id: "B", type: "" }]);
    expect(deleted.map((x) => x.LogicalResourceId)).toEqual(["A"]);
    expect(kept.map((x) => x.LogicalResourceId)).toEqual(["B"]);
  });
  test("says why a delete would not be carried out", () => {
    expect(deleteRefusal({ StackName: "s", EnableTerminationProtection: true }, {})).toContain("Termination protection is on");
    expect(deleteRefusal({ StackName: "s" }, { "e-1": ["user"], "e-2": [] })).toBe("user imports e-1, so DeleteStack leaves s as it is.");
    expect(deleteRefusal({ StackName: "s" }, { "e-1": [] })).toBeNull();
  });
});

describe("events and deleted records", () => {
  test("the first resource that failed is the cause", () => {
    const events = [
      { ResourceStatus: "ROLLBACK_COMPLETE", LogicalResourceId: "s", ResourceType: "AWS::CloudFormation::Stack" },
      { ResourceStatus: "CREATE_FAILED", LogicalResourceId: "Later", ResourceType: "AWS::SQS::Queue", ResourceStatusReason: "Resource creation cancelled" },
      { ResourceStatus: "CREATE_FAILED", LogicalResourceId: "Taken", ResourceType: "AWS::DynamoDB::Table", ResourceStatusReason: "already exists" },
    ];
    expect(failedResource(events)).toEqual({ resource: "Taken", type: "AWS::DynamoDB::Table", reason: "already exists" });
    expect(failedResource([])).toBeNull();
  });
  test("the newest record of a deleted name", () => {
    const list = [{ StackName: "a", StackId: "id-1", DeletionTime: 100 }, { StackName: "a", StackId: "id-2", DeletionTime: 200 }, { StackName: "b", StackId: "id-3", DeletionTime: 300 }];
    expect(deletedRecord(list, "a")).toEqual({ id: "id-2", deleted: "200" });
    expect(deletedRecord(list, "none")).toBeNull();
  });
});
