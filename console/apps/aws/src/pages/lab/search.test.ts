import { describe, expect, test } from "bun:test";
import { labSearch, readInput, tabOf } from "./search";

describe("lab", () => {
  test("the address keeps a service and an operation of plain words only", () => {
    expect(labSearch({ api: "sqs", op: "CreateQueue" })).toEqual({ api: "sqs", op: "CreateQueue" });
    expect(labSearch({ api: "<x>", op: 3 })).toEqual({ api: undefined, op: undefined });
  });

  test("the tab is one of the two, and an address without one shows Rehearse, or Send a call when it names an operation", () => {
    expect(labSearch({ tab: "send" }).tab).toBe("send");
    expect(labSearch({ tab: "rehearse" }).tab).toBe("rehearse");
    expect(labSearch({ tab: "other" }).tab).toBeUndefined();
    expect(tabOf({})).toBe("rehearse");
    expect(tabOf({ tab: "send" })).toBe("send");
    expect(tabOf({ api: "sqs", op: "SendMessage" })).toBe("send");
    expect(tabOf({ tab: "rehearse", api: "sqs" })).toBe("rehearse");
  });

  test("an input is a JSON object, or the reason it is not", () => {
    expect(readInput('{"QueueName": "q"}')).toEqual({ ok: true, value: { QueueName: "q" } });
    expect(readInput("[1]").ok).toBe(false);
    expect(readInput("null").ok).toBe(false);
    const bad = readInput("{");
    expect(bad.ok).toBe(false);
  });
});
