import { describe, expect, test } from "bun:test";
import { splitName } from "./resource-parts";

describe("splitName", () => {
  test("the name is set apart wherever the sentence says it", () => {
    expect(splitName("pick-lists has 2 messages. A message moves to pick-lists-dlq.", "pick-lists")).toEqual([
      { text: "pick-lists", name: true },
      { text: " has 2 messages. A message moves to ", name: false },
      { text: "pick-lists", name: true },
      { text: "-dlq.", name: false },
    ]);
  });
  test("no name, or a sentence without it, is one plain part", () => {
    expect(splitName("Empty.", "")).toEqual([{ text: "Empty.", name: false }]);
    expect(splitName("Empty.", "x")).toEqual([{ text: "Empty.", name: false }]);
  });
});
