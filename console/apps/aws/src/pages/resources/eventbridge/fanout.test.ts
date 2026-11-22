import { describe, expect, test } from "bun:test";
import { fanoutWords } from "./fanout";

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
