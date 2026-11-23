import { describe, expect, test } from "bun:test";
import { pageLine } from "./pagination";

describe("pageLine", () => {
  test("a whole list", () => expect(pageLine(25, 25)).toBe("1 to 25 of 25"));
  test("a list whose total is not asked: what is loaded", () => expect(pageLine(3)).toBe("1 to 3 of 3"));
  test("more to load, total unknown", () => expect(pageLine(50, undefined, true)).toBe("1 to 50, more to load"));
  test("more to load, total known", () => expect(pageLine(50, 1200, true)).toBe("1 to 50 of 1,200"));
  test("empty", () => {
    expect(pageLine(0)).toBe("0 results");
    expect(pageLine(0, undefined, true)).toBe("No results on this page, more to load");
  });
});
