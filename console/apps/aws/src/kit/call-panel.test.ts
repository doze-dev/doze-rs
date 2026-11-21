import { describe, expect, test } from "bun:test";
import { missingWords, viewText } from "./call-panel";

const p = { json: { QueueName: "x" }, cli: "aws sqs create-queue --queue-name x", boto3: "sqs.create_queue(QueueName=\"x\")", missing: [] };

describe("call panel text", () => {
  test("the JSON view names the operation over the body", () => expect(viewText(p, "CreateQueue", "json")).toBe('CreateQueue\n{\n  "QueueName": "x"\n}'));
  test("the other views are the preview's own lines", () => {
    expect(viewText(p, "CreateQueue", "cli")).toBe(p.cli);
    expect(viewText(p, "CreateQueue", "boto3")).toBe(p.boto3);
  });
  test("what is still needed, as words", () => {
    expect(missingWords([])).toBe("");
    expect(missingWords(["QueueName"])).toBe("Needs QueueName.");
    expect(missingWords(["A", "B", "C"])).toBe("Needs A, B and C.");
  });
});

describe("a call to another service", () => {
  const q = { ...p, json: { Action: "Subscribe", Version: "2010-03-31", TopicArn: "t", Protocol: "sqs" } };
  test("a query request is shown by its members, with the service named when asked", () => {
    expect(viewText(q, "Subscribe", "json", "sns")).toBe('sns Subscribe\n{\n  "TopicArn": "t",\n  "Protocol": "sqs"\n}');
    expect(viewText(q, "Subscribe", "json")).toBe('Subscribe\n{\n  "TopicArn": "t",\n  "Protocol": "sqs"\n}');
  });
  test("a body that only looks like one is left whole", () => expect(viewText({ ...p, json: { Action: 1 } }, "X", "json")).toContain('"Action": 1'));
});
