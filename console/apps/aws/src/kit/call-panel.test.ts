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
