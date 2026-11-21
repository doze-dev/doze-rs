import { describe, expect, test } from "bun:test";
import { formOf, patternFields, patternWithForm, testWords } from "./pattern-model";

describe("the pattern step", () => {
  const p = '{"source":["larder.orders"],"detail-type":["OrderPlaced"],"detail":{"total_cents":[{"numeric":[">",0]}]}}';
  test("a pattern names fields", () => {
    expect(patternFields(p)).toEqual(["source", "detail-type", "detail.total_cents"]);
    expect(patternFields("nope")).toEqual([]);
  });
  test("a test says what it checked", () => {
    expect(testWords(true, p)).toBe("The sample event matches the event pattern. It checked source, detail-type, detail.total_cents, and the sample has them all.");
    expect(testWords(false, p)).toContain("does not match");
  });
  test("the form sets source and detail type and keeps the rest", () => {
    expect(JSON.parse(patternWithForm('{"detail":{"a":[1]}}', "a.b, c", "T"))).toEqual({ detail: { a: [1] }, source: ["a.b", "c"], "detail-type": ["T"] });
    expect(JSON.parse(patternWithForm(p, "", ""))).toEqual({ detail: { total_cents: [{ numeric: [">", 0] }] } });
    expect(formOf(p)).toEqual({ source: "larder.orders", detailType: "OrderPlaced" });
    expect(formOf("x")).toEqual({ source: "", detailType: "" });
  });
});
