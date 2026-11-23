import { describe, expect, test } from "bun:test";
import { outlineText } from "./outline";

describe("the call as an outline", () => {
  test("a short member is one line, a long one has its value under its name", () => {
    expect(outlineText("CreateTopic", { Name: "deliveries-updated", Attributes: { DisplayName: "larder deliveries", RawMessageDelivery: false } })).toBe(
      "CreateTopic\n  Name = deliveries-updated\n  Attributes\n    DisplayName\n      larder deliveries\n    RawMessageDelivery = false",
    );
  });
  test("a map is its name over its members", () => expect(outlineText("TagResource", { Tags: { stack: "larder", app: "fulfilment" } })).toBe("TagResource\n  Tags\n    stack = larder\n    app = fulfilment"));
  test("a list is its name over its elements, structures set apart", () => {
    expect(outlineText("X", { Names: ["a", "b"] })).toBe("X\n  Names\n    a\n    b");
    expect(outlineText("X", { Rules: [{ Id: "a", Days: 1 }, { Id: "b", Days: 2 }] })).toBe("X\n  Rules\n    Id = a\n    Days = 1\n\n    Id = b\n    Days = 2");
  });
  test("an empty list or map, numbers, null and a service name", () => {
    expect(outlineText("X", { A: [], B: {}, C: 3, D: null }, "sns")).toBe("sns X\n  A = []\n  B = {}\n  C = 3\n  D = null");
  });
  test("a value of several lines keeps them under its name", () => expect(outlineText("X", { Doc: "a\nb" })).toBe("X\n  Doc\n    a\n    b"));
  test("nothing sent is the operation alone", () => expect(outlineText("X", {})).toBe("X"));
});

describe("tags", () => {
  test("a list of Key and Value structures is drawn as a map", () => {
    expect(outlineText("TagResource", { Tags: [{ Key: "owner", Value: "orders" }] })).toBe("TagResource\n  Tags\n    owner = orders");
    expect(outlineText("X", { Tags: [{ key: "a", value: "b" }] })).toBe("X\n  Tags\n    a = b");
  });
  test("structures with more members stay structures", () => expect(outlineText("X", { L: [{ Key: "a", Value: "b", Extra: "c" }] })).toContain("Extra = c"));
});
