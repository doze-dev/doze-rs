import { describe, expect, test } from "bun:test";
import { fanoutHint, fanoutWords } from "./fanout";

describe("what a topic fans out to", () => {
  test("queues, functions, or subscribers of several kinds", () => {
    expect(fanoutWords([])).toBeNull();
    expect(fanoutWords(["sqs", "sqs", "sqs"])).toBe("3 subscribed queues");
    expect(fanoutWords(["sqs"])).toBe("1 subscribed queue");
    expect(fanoutWords(["lambda", "lambda"])).toBe("2 subscribed functions");
    expect(fanoutWords(["sqs", "email"])).toBe("2 subscribers");
    expect(fanoutWords(["https"])).toBe("1 subscriber");
  });
});

describe("the hint under a chosen topic", () => {
  test("names the queues and functions it sends to, not their ARNs", () => {
    const q = (n: string) => ({ protocol: "sqs", endpoint: `arn:aws:sqs:us-east-1:000000000000:${n}` });
    expect(fanoutHint("orders-created", [q("invoice-queue"), q("inventory-queue"), q("analytics-queue")])).toBe("orders-created already fans out to invoice-queue, inventory-queue and analytics-queue.");
    expect(fanoutHint("t", [{ protocol: "lambda", endpoint: "arn:aws:lambda:us-east-1:000000000000:function:send-email" }, { protocol: "email", endpoint: "ops@larder.test" }])).toBe("t already fans out to send-email and ops@larder.test.");
    expect(fanoutHint("t", [])).toBeNull();
    expect(fanoutHint("t", Array.from({ length: 7 }, (_, i) => q(`q${i}`)))).toBe("t already fans out to q0, q1, q2, q3, q4 and 2 more.");
  });
});
