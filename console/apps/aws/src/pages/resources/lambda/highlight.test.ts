import { describe, expect, test } from "bun:test";
import { tokenize } from "./highlight";

const kinds = (s: string, f: "python" | "node" | "ruby") => tokenize(s, f).filter((t) => t.kind !== "plain").map((t) => `${t.kind}:${t.text}`);

describe("the colours of source code", () => {
  test("never change the text", () => {
    const src = "import json, os\n\n# a comment\ndef f(x):\n    return 'a' + \"b\" + str(12)\n";
    expect(tokenize(src, "python").map((t) => t.text).join("")).toBe(src);
  });
  test("python", () => {
    expect(kinds("import os\nx = 'hi'  # note\n", "python")).toEqual(["kw:import", "str:'hi'", "com:# note"]);
  });
  test("javascript", () => {
    expect(kinds("export const handler = async () => { return 42; } // done", "node")).toEqual(["kw:export", "kw:const", "kw:async", "kw:return", "num:42", "com:// done"]);
  });
  test("ruby", () => {
    expect(kinds("def lambda_handler(event:, context:)\n  nil\nend", "ruby")).toEqual(["kw:def", "kw:nil", "kw:end"]);
  });
});
