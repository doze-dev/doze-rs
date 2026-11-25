import { expect, test } from "bun:test";
import { equalityWords, matchedWords } from "./filter-words";

test("a filter that compares one attribute with one value says what the items have", () => {
  expect(equalityWords("#s = :s", '{"#s": "status"}', '{":s": {"S": "shipped"}}')).toBe("status shipped");
  expect(equalityWords("status=:v", "", '{":v": {"S": "placed"}}')).toBe("status placed");
  expect(equalityWords("#t = :t", '{"#t": "total"}', '{":t": {"N": "19.25"}}')).toBe("total 19.25");
});

test("any other filter is only said to match", () => {
  expect(equalityWords("total > :t", "", '{":t": {"N": "1"}}')).toBeNull();
  expect(equalityWords("#s = :s AND total > :t", '{"#s": "status"}', "{}")).toBeNull();
  expect(equalityWords("#s = :s", "{ not json", '{":s": {"S": "x"}}')).toBeNull();
  expect(equalityWords("#s = :s", '{"#s": "status"}', '{":s": {"BOOL": true}}')).toBeNull();
});

test("the count agrees with the verb", () => {
  expect(matchedWords(4, "#s = :s", '{"#s": "status"}', '{":s": {"S": "shipped"}}')).toBe("4 have status shipped");
  expect(matchedWords(1, "#s = :s", '{"#s": "status"}', '{":s": {"S": "shipped"}}')).toBe("1 has status shipped");
  expect(matchedWords(0, "attribute_exists(x)", "", "")).toBe("0 match the filter");
  expect(matchedWords(1, "attribute_exists(x)", "", "")).toBe("1 matches the filter");
});
