import { describe, expect, test } from "bun:test";
import { callJson, callText, listsOf, missingWords } from "./call-panel";

const p = { json: { QueueName: "x" }, cli: "aws sqs create-queue --queue-name x", boto3: "sqs.create_queue(QueueName=\"x\")", missing: [] };

describe("call panel text", () => {
  test("the JSON drawing names the operation over the body", () => expect(callText(p, "CreateQueue")).toBe('CreateQueue\n{\n  "QueueName": "x"\n}'));
  test("the outline drawing names the operation over its members", () => expect(callText(p, "CreateQueue", { format: "outline" })).toBe("CreateQueue\n  QueueName = x"));
  test("the JSON that is sent has no operation over it", () => expect(callJson(p)).toBe('{\n  "QueueName": "x"\n}'));
  test("what is still needed, as words", () => {
    expect(missingWords([])).toBe("");
    expect(missingWords(["QueueName"])).toBe("Needs QueueName.");
    expect(missingWords(["A", "B", "C"])).toBe("Needs A, B and C.");
  });
});

describe("a call to another service", () => {
  const q = { ...p, json: { Action: "Subscribe", Version: "2010-03-31", TopicArn: "t", Protocol: "sqs" } };
  test("a query request is shown by its members, with the service named when asked", () => {
    expect(callText(q, "Subscribe", { service: "sns" })).toBe('sns Subscribe\n{\n  "TopicArn": "t",\n  "Protocol": "sqs"\n}');
    expect(callText(q, "Subscribe")).toBe('Subscribe\n{\n  "TopicArn": "t",\n  "Protocol": "sqs"\n}');
    expect(callText(q, "Subscribe", { format: "outline" })).toBe("Subscribe\n  TopicArn = t\n  Protocol = sqs");
  });
  test("a body that only looks like one is left whole", () => expect(callText({ ...p, json: { Action: 1 } }, "X")).toContain('"Action": 1'));
});

describe("the JSON as the boards draw it", () => {
  test("a short list of plain values stays on one line, structures are spread", () => {
    expect(callText({ ...p, json: { types: ["REGIONAL"], Tags: [{ Key: "a" }], E: [], O: {} } }, "X")).toBe('X\n{\n  "types": ["REGIONAL"],\n  "Tags": [\n    {\n      "Key": "a"\n    }\n  ],\n  "E": [],\n  "O": {}\n}');
  });
});

describe("lists in the Lambda boards' calls", () => {
  const lam = { ...p, json: { Architectures: ["x86_64"], Methods: ["GET", "HEAD"] } };
  test("a service's boards draw a list of plain values one to a line, the others on one line", () => {
    expect(listsOf("lambda")).toBe("expanded");
    expect(listsOf("s3")).toBe("inline");
    expect(callText(lam, "CreateFunction", { lists: listsOf("lambda") })).toBe('CreateFunction\n{\n  "Architectures": [\n    "x86_64"\n  ],\n  "Methods": [\n    "GET",\n    "HEAD"\n  ]\n}');
    expect(callText(lam, "PutBucketCors", { lists: listsOf("s3") })).toBe('PutBucketCors\n{\n  "Architectures": ["x86_64"],\n  "Methods": ["GET", "HEAD"]\n}');
  });
});

describe("lists of small objects in the CloudFormation and Step Functions boards' calls", () => {
  const cfn = { ...p, json: { StackName: "larder-search", Parameters: [{ ParameterKey: "Stage", ParameterValue: "prod" }, { ParameterKey: "TableName", ParameterValue: "search-index" }], Tags: [{ Key: "app", Value: "larder" }], NotificationARNs: ["arn:aws:sns:us-east-1:123456789012:alerts-of-larder"], Mixed: [{ A: { B: 1 } }] } };
  test("each small flat object is one line; a list of plain values that is too long for one is spread; a nested object is spread", () => {
    expect(listsOf("cloudformation")).toBe("compact");
    expect(listsOf("stepfunctions")).toBe("compact");
    expect(callText(cfn, "CreateStack", { lists: listsOf("cloudformation") })).toBe(
      'CreateStack\n{\n  "StackName": "larder-search",\n  "Parameters": [\n    { "ParameterKey": "Stage", "ParameterValue": "prod" },\n    { "ParameterKey": "TableName", "ParameterValue": "search-index" }\n  ],\n  "Tags": [\n    { "Key": "app", "Value": "larder" }\n  ],\n  "NotificationARNs": [\n    "arn:aws:sns:us-east-1:123456789012:alerts-of-larder"\n  ],\n  "Mixed": [\n    {\n      "A": {\n        "B": 1\n      }\n    }\n  ]\n}',
    );
  });
  test("an object too long for a line is spread, and every other service keeps spreading its objects", () => {
    const long = { ...p, json: { Tags: [{ Key: "k", Value: "v".repeat(120) }] } };
    expect(callText(long, "CreateStack", { lists: "compact" })).toContain('{\n      "Key": "k"');
    expect(callText({ ...p, json: { Tags: [{ Key: "a", Value: "b" }] } }, "TagQueue", { lists: listsOf("sqs") })).toBe('TagQueue\n{\n  "Tags": [\n    {\n      "Key": "a",\n      "Value": "b"\n    }\n  ]\n}');
  });
});
