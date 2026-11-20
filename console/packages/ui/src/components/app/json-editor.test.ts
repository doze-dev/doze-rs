import { describe, expect, test } from "bun:test";
import { checkJson, formatJson } from "./json-editor";

describe("the JSON editor's check", () => {
  test("says where a document is not JSON, and what empty means", () => {
    expect(checkJson('{"a":1}')).toEqual({ ok: true, value: { a: 1 } });
    expect(checkJson("  ")).toEqual({ ok: true, value: undefined });
    expect(checkJson("", "invalid").ok).toBe(false);
    const bad = checkJson("{a:1}");
    expect(bad.ok).toBe(false);
    expect(!bad.ok && bad.why.length).toBeGreaterThan(0);
  });

  test("formats JSON, and leaves what is not as it is", () => {
    expect(formatJson('{"a":[1,2]}')).toBe('{\n  "a": [\n    1,\n    2\n  ]\n}');
    expect(formatJson("{a")).toBe("{a");
    expect(formatJson("")).toBe("");
  });
});
