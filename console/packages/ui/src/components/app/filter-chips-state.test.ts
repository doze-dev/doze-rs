import { describe, expect, test } from "bun:test";
import { chipValue, chipWords } from "./filter-chips-state";

const chips = [{ value: "", label: "All", count: 12 }, { value: "ALARM", label: "In alarm", count: 2 }, { value: "OK", label: "OK" }];
describe("filter chips", () => {
  test("a value that names a chip is kept, any other is the first", () => {
    expect(chipValue("ALARM", chips)).toBe("ALARM");
    expect(chipValue("nope", chips)).toBe("");
    expect(chipValue(undefined, chips)).toBe("");
    expect(chipValue("x", [])).toBe("");
  });
  test("words carry the count when there is one", () => {
    expect(chipWords(chips[1]!)).toBe("In alarm 2");
    expect(chipWords(chips[2]!)).toBe("OK");
    expect(chipWords({ value: "x", label: "Big", count: 12000 })).toBe("Big 12,000");
  });
});
