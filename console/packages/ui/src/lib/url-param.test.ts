import { describe, expect, test } from "bun:test";
import { paramText, withParam } from "./url-param";

describe("url param", () => {
  test("sets and removes one key, keeping the others", () => {
    expect(withParam({ a: "1", tab: "x" }, "tab", "y")).toEqual({ a: "1", tab: "y" });
    expect(withParam({ a: "1", tab: "x" }, "tab", "")).toEqual({ a: "1" });
    expect(withParam({ a: "1" }, "tab", undefined)).toEqual({ a: "1" });
  });
  test("a value the address read as another type is text again", () => {
    expect(paramText(12)).toBe("12");
    expect(paramText(true)).toBe("true");
    expect(paramText(["a"])).toBeUndefined();
    expect(paramText(undefined)).toBeUndefined();
  });
});
