import { describe, expect, test } from "bun:test";
import { splitName } from "./resource-parts";

describe("splitName", () => {
  test("the name is set apart wherever the sentence says it, and whole", () => {
    // `pick-lists-dlq` is another name: the one name does not reach into it (the lede once read **pick-lists**-dlq).
    expect(splitName("pick-lists has 2 messages. A message moves to pick-lists-dlq.", "pick-lists")).toEqual([
      { text: "pick-lists", name: true },
      { text: " has 2 messages. A message moves to pick-lists-dlq.", name: false },
    ]);
    expect(splitName("pick-lists-dlq holds 1 message, the dead-letter queue of pick-lists.", "pick-lists-dlq").filter((p) => p.name)).toEqual([{ text: "pick-lists-dlq", name: true }]);
  });
  test("no name, or a sentence without it, is one plain part", () => {
    expect(splitName("Empty.", "")).toEqual([{ text: "Empty.", name: false }]);
    expect(splitName("Empty.", "x")).toEqual([{ text: "Empty.", name: false }]);
  });
});
