import { describe, expect, test } from "bun:test";
import { placeholders, select, transform, validPath } from "./transform";

const event = { "detail-type": "OrderPlaced", detail: { order_id: "o-1", customer_id: "c-1", total_cents: 4850, items: [{ id: 7 }, { id: 8 }], "a-b": "x" } };

describe("paths", () => {
  test("by dot and bracket", () => {
    expect(select(event, "$.detail.items[1].id")).toBe(8);
    expect(select(event, "$['detail']['a-b']")).toBe("x");
    expect(select(event, '$["detail-type"]')).toBe("OrderPlaced");
    expect(select(event, "$.detail.missing")).toBeUndefined();
    expect(select(event, "$.detail.items[5]")).toBeUndefined();
  });
  test("those EventBridge refuses", () => {
    for (const bad of ["detail", "$.", "$.a.*", "$..a", "$[", "$[x]", ""]) expect(validPath(bad)).toBe(false);
    expect(validPath("$")).toBe(true);
  });
});

describe("a template", () => {
  const paths = { order_id: "$.detail.order_id", customer: "$.detail.customer_id", total: "$.detail.total_cents", gone: "$.detail.nope" };
  test("fills what the board shows", () => {
    expect(transform(event, paths, '{"order_id": "<order_id>", "customer": "<customer>"}')).toEqual({ output: '{"order_id": "o-1", "customer": "c-1"}' });
  });
  test("outside a string text is quoted and a number is a number; nothing is null", () => {
    expect(transform(event, paths, '{"order_id": <order_id>, "total": <total>, "gone": <gone>}')).toEqual({ output: '{"order_id": "o-1", "total": 4850, "gone": null}' });
  });
  test("a text template takes text as it is, and a missing path is empty inside a string", () => {
    expect(transform(event, paths, "order <order_id> for <customer>")).toEqual({ output: "order o-1 for c-1" });
    expect(transform(event, paths, '{"x": "<gone>"}')).toEqual({ output: '{"x": ""}' });
  });
  test("what cannot be filled", () => {
    expect(transform(event, paths, "<nobody>")).toEqual({ problem: "The template names <nobody>, which no input path defines." });
    expect(transform(event, { a: "detail" }, "<a>")).toEqual({ problem: "The path for a is not valid: detail" });
    expect(transform(event, paths, '{"a": <order_id}')).toEqual({ problem: expect.stringContaining("The output is not JSON") as unknown as string });
  });
  test("the predefined names", () => {
    expect(transform(event, {}, '{"rule": "<aws.events.rule-name>"}', { ruleName: "order-placed" })).toEqual({ output: '{"rule": "order-placed"}' });
    expect(placeholders("<a> and <b> and <a> < c>")).toEqual(["a", "b"]);
  });
});
