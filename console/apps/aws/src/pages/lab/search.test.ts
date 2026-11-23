import { describe, expect, test } from "bun:test";
import { labSearch, readInput } from "./search";

describe("lab", () => {
  test("the address keeps a service and an operation of plain words only", () => {
    expect(labSearch({ api: "sqs", op: "CreateQueue" })).toEqual({ api: "sqs", op: "CreateQueue" });
    expect(labSearch({ api: "<x>", op: 3 })).toEqual({ api: undefined, op: undefined });
  });

  test("an input is a JSON object, or the reason it is not", () => {
    expect(readInput('{"QueueName": "q"}')).toEqual({ ok: true, value: { QueueName: "q" } });
    expect(readInput("[1]").ok).toBe(false);
    expect(readInput("null").ok).toBe(false);
    const bad = readInput("{");
    expect(bad.ok).toBe(false);
  });
});
