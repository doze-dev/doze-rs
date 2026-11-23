import { describe, expect, test } from "bun:test";
import { deletedSaid, exportsSaid, makesWords, stackSaid, updatedSaid, type StackFacts } from "./sentences";

const res = (type: string, status = "CREATE_COMPLETE") => ({ LogicalResourceId: type, ResourceType: type, ResourceStatus: status });
const base: StackFacts = { name: "larder-core", status: "CREATE_COMPLETE", created: "2026-11-06 18:40:12 UTC", updated: null, resources: [res("AWS::SQS::Queue"), res("AWS::SNS::Topic"), res("AWS::S3::Bucket")], exports: ["e-1", "e-2"], importers: { "e-1": [], "e-2": [] }, failed: null };

describe("a stack's sentence", () => {
  test("a stable stack says what it owns and whether anything stops a delete", () => {
    expect(stackSaid(base).text).toBe("larder-core owns 3 resources in 3 services and has been stable since 2026-11-06 18:40:12 UTC. No stack imports its two exports, so nothing stops you deleting it.");
  });
  test("an importer is named, and what it blocks", () => {
    const s = stackSaid({ ...base, exports: ["e-1"], importers: { "e-1": ["larder-edge"] } });
    expect(s.text).toContain("larder-edge imports e-1, so larder-core can't be deleted while it does.");
    expect(s.names).toContain("larder-edge");
  });
  test("an update says since when", () => {
    expect(stackSaid({ ...base, updated: "T2", exports: [] }).text).toBe("larder-core owns 3 resources in 3 services and has been stable since T2.");
  });
  test("a stack waiting for its change set says what it would add, never that it is being worked on", () => {
    const s = stackSaid({ ...base, name: "s", status: "REVIEW_IN_PROGRESS", resources: [], waiting: { name: "initial", adds: [{ logical: "T", type: "AWS::DynamoDB::Table" }] } });
    expect(s.text).toContain("s is waiting for review. Its change set initial would add 1 resource: T (DynamoDB table).");
    expect(s.text).not.toContain("being worked on");
  });
  test("a create that was rolled back holds nothing and names the resource that failed", () => {
    const s = stackSaid({ ...base, name: "s", status: "ROLLBACK_COMPLETE", resources: [], failed: { resource: "Taken", type: "AWS::SSM::Parameter", reason: "already exists" } });
    expect(s.text).toBe("s failed to create and CloudFormation undid it. Taken could not be made: already exists. It holds no resources.");
    expect(s.text).not.toContain("held before");
  });
  test("a stack in progress says it follows", () => {
    expect(stackSaid({ ...base, status: "UPDATE_IN_PROGRESS" }).text).toContain("is being worked on (UPDATE_IN_PROGRESS)");
  });
  test("a rolled-back update holds what it held before", () => {
    expect(stackSaid({ ...base, status: "UPDATE_ROLLBACK_COMPLETE" }).text).toContain("holds what it held before that update");
  });
  test("a deleted stack says what was kept", () => {
    expect(deletedSaid({ ...base, retained: ["product-images"] }).text).toBe("larder-core is gone and what it owned with it. product-images was kept, as its template asked.");
  });
});

describe("what an update says", () => {
  test("names the resources the last update made and changed, not those of the creation", () => {
    const stack = "AWS::CloudFormation::Stack";
    const events = [
      { LogicalResourceId: "s", ResourceType: stack, ResourceStatus: "UPDATE_COMPLETE" },
      { LogicalResourceId: "Queue", ResourceType: "AWS::SQS::Queue", ResourceStatus: "UPDATE_COMPLETE" },
      { LogicalResourceId: "ReportsBucket", ResourceType: "AWS::S3::Bucket", ResourceStatus: "CREATE_COMPLETE" },
      { LogicalResourceId: "s", ResourceType: stack, ResourceStatus: "UPDATE_IN_PROGRESS" },
      { LogicalResourceId: "Old", ResourceType: "AWS::SQS::Queue", ResourceStatus: "CREATE_COMPLETE" },
    ];
    expect(updatedSaid("s", events).text).toBe("s was updated: ReportsBucket was created and Queue was changed.");
    expect(updatedSaid("s", []).text).toBe("s was updated.");
  });
});

describe("the exports' sentence", () => {
  const e = [{ name: "a-x", stack: "a" }, { name: "a-y", stack: "a" }, { name: "b-z", stack: "b" }];
  test("nothing imported", () => expect(exportsSaid(e, {}).text).toBe("3 exports from 2 stacks. Nothing imports them today."));
  test("one imported", () => expect(exportsSaid(e, { "b-z": ["c"] }).text).toBe("3 exports from 2 stacks. Only one is imported: c imports b-z, so b can't be deleted while it does."));
  test("a single export", () => expect(exportsSaid([e[0]!], {}).text).toBe("1 export from 1 stack. Nothing imports it today."));
});

describe("what a template makes", () => {
  test("by name when every type's logical IDs are known, else by how many types", () => {
    expect(makesWords(["AWS::DynamoDB::Table", "AWS::SQS::Queue"], [{ ResourceType: "AWS::DynamoDB::Table", LogicalResourceIds: ["SearchIndexTable"] }, { ResourceType: "AWS::SQS::Queue", LogicalResourceIds: ["SearchReindexQueue"] }])).toBe("SearchIndexTable (DynamoDB table) and SearchReindexQueue (SQS queue)");
    expect(makesWords(["AWS::SQS::Queue"], [{ ResourceType: "AWS::SQS::Queue", LogicalResourceIds: ["A", "B", "C"] }])).toBe("A (SQS queue), B (SQS queue) and C (SQS queue)");
    // A type the summary names no resource of: the count of types is what is known.
    expect(makesWords(["AWS::SQS::Queue", "AWS::Lambda::Function"], [{ ResourceType: "AWS::SQS::Queue", LogicalResourceIds: ["Q"] }])).toBe("2 resource types");
    expect(makesWords(["AWS::SQS::Queue"], undefined)).toBe("1 resource type");
  });
});
