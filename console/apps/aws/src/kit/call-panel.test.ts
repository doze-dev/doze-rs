import { describe, expect, test } from "bun:test";
import { callJson, callText, missingWords } from "./call-panel";

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
