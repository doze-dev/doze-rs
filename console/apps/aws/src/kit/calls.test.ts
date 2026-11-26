import { describe, expect, test } from "bun:test";
import type { KeyPrincipal } from "../api/types/KeyPrincipal";
import { callerWords, isConsoleRead, isReadCall, namedKeys, targetKey, tookWords } from "./calls";

const titles = new Map([["events", "EventBridge"], ["sns", "SNS"]]);
const user: KeyPrincipal = { key: "AKIAUSER", kind: "user", name: "admin-cli", arn: "arn:aws:iam::1:user/admin-cli" };
const names = new Map([["AKIAUSER", user]]);

describe("callerWords", () => {
  test("a call a service made names the service and what it made it for", () => {
    const c = { caller: "AKIAX", sdk: null, via: { by: "events", name: "rule payment-settled", request_id: null } };
    expect(callerWords(c, titles, names)).toMatchObject({ label: "EventBridge rule payment-settled", key: null });
    expect(callerWords({ ...c, via: { by: "sns", name: null, request_id: null } }, titles, names).label).toBe("SNS");
    expect(callerWords({ ...c, via: { by: null, name: null, request_id: null } }, titles, names).label).toBe("A service, as the caller's role");
  });
  test("an IAM user is shown by name, any other key by its client with the key beside it", () => {
    expect(callerWords({ caller: "AKIAUSER", sdk: "AWS CLI 2.15.30", via: null }, titles, names)).toMatchObject({ label: "admin-cli", key: null });
    expect(callerWords({ caller: "AKIAOTHER", sdk: "boto3 1.34.69", via: null }, titles, names)).toMatchObject({ label: "boto3", key: "AKIAOTHER" });
    expect(callerWords({ caller: "AKIAOTHER", sdk: null, via: null }, titles, names)).toMatchObject({ label: "AKIAOTHER", key: null });
  });
  test("the console and a call with no key say so", () => {
    expect(callerWords({ caller: "console", sdk: null, via: null }, titles, names).label).toBe("This console");
    expect(callerWords({ caller: null, sdk: null, via: null }, titles, names).label).toBe("–");
    // The console's calls ride the bus, so they carry a via with no service; a service acting for the console is still a service.
    expect(callerWords({ caller: "console", sdk: null, via: { by: null, name: null, request_id: null } }, titles, names).label).toBe("This console");
    expect(callerWords({ caller: "console", sdk: null, via: { by: "sns", name: null, request_id: null } }, titles, names).label).toBe("SNS");
  });
});

test("a call between services has no time of its own; a wire call says how long it took", () => {
  expect(tookWords({ duration_us: 2400, request_id: "r-1" })).toBe("2.4 ms");
  expect(tookWords({ duration_us: 800, request_id: "" })).toBe("");
});

test("only the access keys of the wire are asked about", () => {
  const via = { by: "sns", name: null, request_id: null };
  expect(namedKeys([{ caller: "AKIAB", via: null }, { caller: "AKIAA", via: null }, { caller: "AKIAB", via: null }, { caller: "console", via: null }, { caller: "AKIAC", via }])).toBe("AKIAA,AKIAB");
});

test("an S3 call that named an object shows its key beside the bucket's page; any other call shows none", () => {
  const s3 = (resource: string | null, member: string | null = null) => ({ service: "s3", member, resource });
  expect(targetKey(s3("pics/a/one.txt"))).toBe("a/one.txt");
  expect(targetKey(s3("pics"))).toBeNull();
  expect(targetKey(s3("pics/"))).toBeNull();
  expect(targetKey(s3(null))).toBeNull();
  // A call that names the bucket by a member (a listing's `Bucket`) acts on the bucket.
  expect(targetKey(s3("pics", "Bucket"))).toBeNull();
  expect(targetKey({ service: "sqs", member: null, resource: "a/b" })).toBeNull();
});

test("an operation that only looks is a read; one that changes anything, or starts something, is not", () => {
  for (const a of ["ListQueues", "DescribeTable", "GetFunction", "HeadObject", "Query", "Scan", "BatchGetItem", "FilterLogEvents", "GetSecretValue", "ListMessageMoveTasks"]) expect(isReadCall({ action: a })).toBe(true);
  for (const a of ["CreateQueue", "PutObject", "DeleteBucket", "StartMessageMoveTask", "SendMessage", "ReceiveMessage", "Publish", "UpdateFunctionCode", "StartExecution", "ExecuteStatement"]) expect(isReadCall({ action: a })).toBe(false);
  expect(isReadCall({ action: null })).toBe(false);
});

test("only the console's own reads are hidden: its writes, an app's reads and a pinned read stay", () => {
  expect(isConsoleRead({ caller: "console", action: "ListQueues" })).toBe(true);
  expect(isConsoleRead({ caller: "console", action: "StartMessageMoveTask" })).toBe(false);
  expect(isConsoleRead({ caller: "console", action: "CreateQueue" })).toBe(false);
  expect(isConsoleRead({ caller: "AKIAAPP", action: "ListQueues" })).toBe(false);
  expect(isConsoleRead({ caller: "console", action: "GetSecretValue" }, ["GetSecretValue"])).toBe(false);
  expect(isConsoleRead({ caller: "console", action: "GetSecretValue" }, ["GetParameter"])).toBe(true);
});
