import { describe, expect, test } from "bun:test";
import { allowPolicy, attributes, basicPolicy, createInput, emptyForm, formOf, nameProblem, problems } from "./form-model";
import type { Queue } from "./queue-model";

const ARN = "arn:aws:sqs:us-east-1:1:pick-lists";
const queue = (over: Partial<Queue> = {}): Queue => ({ name: "pick-lists", fifo: false, url: "u", arn: ARN, available: 0, inFlight: 0, delayed: 0, dlq: null, maxReceives: null, visibility: 30, retention: 345_600, delay: 0, maxSize: 1_048_576, wait: 0, dedup: false, sse: "sqs", kms: null, reuse: null, scope: null, throughput: null, created: null, modified: null, policy: null, allow: null, ...over });

describe("a new queue", () => {
  test("a form of the defaults sends only the name", () => {
    expect(createInput({ ...emptyForm(), name: "pick-lists" }, ARN)).toEqual({ QueueName: "pick-lists" });
  });
  test("only what differs from SQS's defaults is sent, in seconds", () => {
    const f = { ...emptyForm(), name: "q", visibility: { value: "2", unit: "minutes" as const }, retention: { value: "1", unit: "days" as const } };
    expect(createInput(f, ARN)).toEqual({ QueueName: "q", Attributes: { VisibilityTimeout: "120", MessageRetentionPeriod: "86400" } });
  });
  test("a FIFO queue says so, and high throughput sets the two attributes it means", () => {
    const f = { ...emptyForm(), name: "q.fifo", fifo: true, dedup: true, highThroughput: true, tags: [{ id: "1", key: "app", value: "x" }] };
    expect(createInput(f, ARN)).toEqual({ QueueName: "q.fifo", Attributes: { FifoQueue: "true", ContentBasedDeduplication: "true", DeduplicationScope: "messageGroup", FifoThroughputLimit: "perMessageGroupId" }, tags: { app: "x" } });
  });
  test("encryption, the dead-letter queue and the allow policy are attributes", () => {
    const f = { ...emptyForm(), name: "q", sse: "kms" as const, kms: "alias/app", reuse: { value: "10", unit: "minutes" as const }, dlqOn: true, dlq: "arn:dlq", maxReceives: "3", allowOn: true, allowMode: "denyAll" as const };
    expect(createInput(f, ARN)).toEqual({ QueueName: "q", Attributes: { KmsMasterKeyId: "alias/app", KmsDataKeyReusePeriodSeconds: "600", RedrivePolicy: JSON.stringify({ deadLetterTargetArn: "arn:dlq", maxReceiveCount: 3 }), RedriveAllowPolicy: JSON.stringify({ redrivePermission: "denyAll" }) } });
    expect(allowPolicy({ allowOn: true, allowMode: "byQueue", allowArns: "a\nb, c" })).toBe(JSON.stringify({ redrivePermission: "byQueue", sourceQueueArns: ["a", "b", "c"] }));
  });
});

describe("the rules", () => {
  test("a name", () => {
    expect(nameProblem("", false)).toContain("name");
    expect(nameProblem("a b", false)).toContain("letters");
    expect(nameProblem("orders", true)).toContain(".fifo");
    expect(nameProblem("orders.fifo", true)).toBeNull();
    expect(nameProblem("x".repeat(81), false)).toContain("80");
  });
  test("each setting keeps to SQS's range", () => {
    const f = { ...emptyForm(), name: "q", visibility: { value: "13", unit: "hours" as const }, wait: { value: "21", unit: "seconds" as const }, maxKib: "2000" };
    expect(Object.keys(problems(f, { create: true })).sort()).toEqual(["maxKib", "visibility", "wait"]);
    expect(problems({ ...emptyForm(), name: "q", dlqOn: true }, { create: true }).dlq).toContain("Choose");
  });
});

describe("an edit", () => {
  test("a form of the queue as it is sends nothing", () => {
    const q = queue({ dlq: "dlq", maxReceives: 5, policy: '{"Version":"2012-10-17","Statement":[]}', visibility: 60 });
    const dlqArn = "arn:aws:sqs:us-east-1:1:dlq";
    expect(attributes(formOf(q, [], dlqArn), { create: false, queue: q, dlqArn }, ARN)).toEqual({});
  });
  test("a change is one attribute, and turning the dead-letter queue off empties it", () => {
    const q = queue({ dlq: "dlq", maxReceives: 5 });
    const dlqArn = "arn:aws:sqs:us-east-1:1:dlq";
    const f = { ...formOf(q, [], dlqArn), visibility: { value: "60", unit: "seconds" as const }, dlqOn: false };
    expect(attributes(f, { create: false, queue: q, dlqArn }, ARN)).toEqual({ VisibilityTimeout: "60", RedrivePolicy: "" });
  });
  test("moving from SSE-KMS to SSE-SQS clears the key", () => {
    const q = queue({ sse: "kms", kms: "alias/a", reuse: 300 });
    const f = { ...formOf(q, [], null), sse: "sqs" as const };
    expect(attributes(f, { create: false, queue: q, dlqArn: null }, ARN)).toEqual({ KmsMasterKeyId: "", SqsManagedSseEnabled: "true" });
  });
});

describe("the basic access policy", () => {
  test("the owner needs no statement; others are written as statements", () => {
    expect(basicPolicy({ send: "owner", receive: "owner", principals: "" }, ARN)).toBeNull();
    const p = JSON.parse(basicPolicy({ send: "accounts", receive: "everyone", principals: "123456789012" }, ARN)!) as { Statement: { Principal: unknown; Action: unknown }[] };
    expect(p.Statement[0]).toMatchObject({ Principal: { AWS: ["arn:aws:iam::123456789012:root"] }, Action: "sqs:SendMessage" });
    expect(p.Statement[1]).toMatchObject({ Principal: "*", Action: ["sqs:ReceiveMessage", "sqs:DeleteMessage", "sqs:GetQueueAttributes"] });
  });
});
