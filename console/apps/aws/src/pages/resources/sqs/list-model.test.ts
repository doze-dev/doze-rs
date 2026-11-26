import { expect, test } from "bun:test";
import { createdWords } from "./list-model";

test("a queue's creation is said to the minute, with the whole time to hand", () => {
  const ms = Date.UTC(2026, 10, 6, 18, 42, 9);
  expect(createdWords(ms)).toEqual({ short: "2026-11-06 18:42", full: "2026-11-06 18:42:09 UTC" });
  expect(createdWords(null)).toBeNull();
});
