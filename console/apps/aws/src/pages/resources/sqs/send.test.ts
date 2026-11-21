import { describe, expect, test } from "bun:test";
import { sendInput } from "./send";
import { changed } from "./settings";

describe("SQS", () => {
  const form = { body: "hi", delay: "", group: "", dedup: "", attrs: [] };
  test("a send is its body, its delay or its FIFO ids, and its attributes", () => {
    expect(sendInput({ ...form, delay: "5", attrs: [{ name: "k", type: "Number", value: "2" }] }, false)).toEqual({ MessageBody: "hi", DelaySeconds: 5, MessageAttributes: { k: { DataType: "Number", StringValue: "2" } } });
    expect(sendInput({ ...form, delay: "901" }, false)).toContain("0 to 900");
    expect(sendInput(form, true)).toContain("message group ID");
    expect(sendInput({ ...form, group: "g", dedup: "d" }, true)).toEqual({ MessageBody: "hi", MessageGroupId: "g", MessageDeduplicationId: "d" });
    expect(sendInput({ ...form, body: "" }, false)).toContain("body");
    expect(sendInput({ ...form, attrs: [{ name: "n", type: "Number", value: "x" }] }, false)).toContain("Number");
  });

  test("settings send only what changed, a dead-letter queue as its ARN", () => {
    const was = { VisibilityTimeout: "30", MessageRetentionPeriod: "345600", DelaySeconds: "0", ReceiveMessageWaitTimeSeconds: "0", MaximumMessageSize: "262144", dlq: "", maxReceives: "", dedup: "false" };
    const arn = (q: string) => `arn:aws:sqs:us-east-1:000000000000:${q}`;
    expect(changed(was, was, arn, false)).toEqual({});
    expect(changed(was, { ...was, VisibilityTimeout: "45" }, arn, false)).toEqual({ VisibilityTimeout: "45" });
    expect(changed(was, { ...was, VisibilityTimeout: "50000" }, arn, false)).toContain("43,200");
    expect(changed(was, { ...was, dlq: "dlq", maxReceives: "3" }, arn, false)).toEqual({ RedrivePolicy: JSON.stringify({ deadLetterTargetArn: arn("dlq"), maxReceiveCount: 3 }) });
    expect(changed({ ...was, dlq: "dlq", maxReceives: "3" }, was, arn, false)).toEqual({ RedrivePolicy: "" });
    expect(changed(was, { ...was, dedup: "true" }, arn, true)).toEqual({ ContentBasedDeduplication: "true" });
  });
});
