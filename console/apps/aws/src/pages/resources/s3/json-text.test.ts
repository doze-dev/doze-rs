import { describe, expect, test } from "bun:test";
import { readableJson, readableText } from "./json-text";

describe("readableJson", () => {
  test("a list of plain values stays on one line, a list of objects does not", () => {
    expect(readableJson([{ AllowedMethods: ["GET", "HEAD"], MaxAgeSeconds: 3000 }])).toBe('[\n  {\n    "AllowedMethods": ["GET", "HEAD"],\n    "MaxAgeSeconds": 3000\n  }\n]');
  });
  test("a policy given as one line of text is written a member a line", () => {
    const one = '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":["s3:GetObject"],"Resource":"arn:aws:s3:::b/*"}]}';
    expect(readableText(one).split("\n")).toHaveLength(10);
    expect(readableText(one)).toContain('"Action": ["s3:GetObject"],');
  });
  test("empty things and text that is not JSON", () => {
    expect(readableJson({ a: {}, b: [] })).toBe('{\n  "a": {},\n  "b": []\n}');
    expect(readableText("not json")).toBe("not json");
  });
});
