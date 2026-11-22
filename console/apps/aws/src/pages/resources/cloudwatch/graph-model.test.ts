import { describe, expect, test } from "bun:test";
import { type GM, decode, encode, metricWidget, nextId, sourceOf, summaryLine, toQueries } from "./graph-model";

const placed: GM = { id: "m1", ns: "Larder/Orders", m: "OrdersPlaced", d: [{ Name: "Environment", Value: "prod" }], stat: "Sum", period: 300, label: "OrdersPlaced", shown: true };
const failed: GM = { ...placed, id: "m2", m: "OrdersFailed", label: "OrdersFailed" };

describe("graphed metrics", () => {
  test("they go through the address and back", () => {
    expect(decode(encode([placed, failed]))).toEqual([placed, failed].map((r) => ({ ...r, expr: undefined })));
    expect(decode("not json")).toEqual([]);
    expect(decode(JSON.stringify([{ id: 1 }]))).toEqual([]);
  });
  test("ids", () => {
    expect(nextId([placed, failed])).toBe("m3");
    expect(nextId([placed], true)).toBe("e1");
  });
  test("the summary line is the board's", () => {
    expect(summaryLine([placed, failed], "last 3 hours")).toBe("2 metrics from Larder/Orders, 5 minute periods, last 3 hours.");
    expect(summaryLine([], "last 3 hours")).toBe("");
  });
  test("a hidden metric that feeds a shown expression is asked for and not drawn", () => {
    const math: GM = { id: "e1", stat: "", period: 0, label: "Failure rate", shown: true, expr: "m2 / m1 * 100" };
    const q = toQueries([{ ...placed, shown: false }, { ...failed, shown: false }, math]);
    expect(q.map((x) => [x.id, x.hide ?? false])).toEqual([["m1", true], ["m2", true], ["e1", false]]);
    expect(toQueries([{ ...placed, shown: false }])).toEqual([]);
  });
  test("the widget they make is a dashboard's metric widget", () => {
    const w = metricWidget([placed], { view: "timeSeries", region: "us-east-1" }) as { type: string; properties: { metrics: unknown[][]; period: number } };
    expect(w.type).toBe("metric");
    expect(w.properties.metrics[0]).toEqual(["Larder/Orders", "OrdersPlaced", "Environment", "prod", { stat: "Sum", period: 300, id: "m1" }]);
    expect(JSON.parse(sourceOf([placed], "us-east-1")).region).toBe("us-east-1");
  });
});
