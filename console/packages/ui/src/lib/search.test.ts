import { expect, test } from "bun:test";
import { oneOf, shortText } from "./search";

test("oneOf keeps only an allowed word", () => {
  const tabs = ["messages", "config"] as const;
  expect(oneOf(tabs, "config")).toBe("config");
  expect(oneOf(tabs, "other")).toBeUndefined();
  expect(oneOf(tabs, ["config"])).toBeUndefined();
  expect(oneOf(tabs, 1)).toBeUndefined();
});

test("shortText keeps a text that is not empty and not long", () => {
  expect(shortText("orders")).toBe("orders");
  expect(shortText("")).toBeUndefined();
  expect(shortText("x".repeat(65))).toBeUndefined();
  expect(shortText("x".repeat(65), 80)).toBe("x".repeat(65));
  expect(shortText(12)).toBeUndefined();
});
