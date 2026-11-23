import { describe, expect, test } from "bun:test";
import { blocks, inline } from "./markdown-parse";

describe("the Markdown of a text widget", () => {
  test("headings, paragraphs and lists", () => {
    const b = blocks("# Runbook\n\nOpen `pick-lists-dlq`, then **redrive** it.\nSecond line.\n\n- one\n- two\n");
    expect(b.map((x) => x.t)).toEqual(["h", "p", "ul"]);
    expect(b[1]).toMatchObject({ t: "p" });
    expect((b[2] as { items: unknown[] }).items.length).toBe(2);
  });
  test("inline marks, and only http links", () => {
    expect(inline("a **b** `c` *d* [e](https://x.test/y)").map((x) => x.t)).toEqual(["text", "b", "text", "code", "text", "i", "text", "a"]);
    expect(inline("[x](javascript:alert(1))")).toEqual([{ t: "text", v: "[x](javascript:alert(1))" }]);
  });
  test("markup is text, never HTML", () => {
    expect(inline("<script>alert(1)</script>")).toEqual([{ t: "text", v: "<script>alert(1)</script>" }]);
  });
  test("fenced code", () => {
    expect(blocks("```\nx = 1\n```")).toEqual([{ t: "pre", v: "x = 1" }]);
  });
});
