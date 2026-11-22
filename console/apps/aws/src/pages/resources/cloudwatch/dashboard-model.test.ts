import { describe, expect, test } from "bun:test";
import { alarmNames, parseBody, place, queriesOf, rulesBroken, textWidget, titleOf, withWidget, withoutWidget } from "./dashboard-model";

const body = parseBody(JSON.stringify({ start: "-PT3H", widgets: [{ type: "metric", x: 0, y: 0, width: 12, height: 6, properties: { title: "Orders placed", stat: "Sum", period: 300, metrics: [["Larder/Orders", "OrdersPlaced", "Environment", "prod"], ["...", "OrdersFailed", "Environment", "prod", { label: "Failed", stat: "Maximum" }], [{ expression: "m1 / 2", label: "Half", id: "e1" }]] } }, { type: "alarm", properties: { alarms: ["arn:aws:cloudwatch:us-east-1:111122223333:alarm:dlq-not-empty"] } }] }))!;

describe("a dashboard body", () => {
  test("only a JSON object is a body, and a body keeps what else it says", () => {
    expect(parseBody("[]")).toBeNull();
    expect(parseBody("nope")).toBeNull();
    expect(parseBody("{}")?.widgets).toEqual([]);
    expect(body.start).toBe("-PT3H");
  });
  test("a metric widget's rows are queries, '...' repeating the namespace above and options overriding the widget's", () => {
    const q = queriesOf(body.widgets[0]!);
    expect(q.map((x) => [x.namespace, x.metric, x.stat, x.label])).toEqual([["Larder/Orders", "OrdersPlaced", "Sum", "OrdersPlaced"], ["Larder/Orders", "OrdersFailed", "Maximum", "Failed"], ["", "", undefined, "Half"]]);
    expect(q[2]!.expression).toBe("m1 / 2");
    expect(q[1]!.dimensions).toEqual({ Environment: "prod" });
  });
  test("a widget with no x and y takes the next free place", () => {
    const placed = place(withWidget(body, textWidget("# Runbook", "solid")).widgets);
    expect(placed.map((p) => [p.x, p.y])).toEqual([[0, 0], [12, 0], [18, 0]]);
    expect(placed.length).toBe(3);
  });
  test("a widget is taken out by its place in the list", () => {
    expect(withoutWidget(body, 0).widgets.length).toBe(1);
  });
  test("names and titles", () => {
    expect(alarmNames(body.widgets[1]!)).toEqual(["dlq-not-empty"]);
    expect(titleOf(body.widgets[0]!)).toBe("Orders placed");
    expect(titleOf(body.widgets[1]!)).toBe("Alarm status");
  });
  test("what breaks CloudWatch's rules is said", () => {
    expect(rulesBroken(body)).toEqual([]);
    expect(rulesBroken({ widgets: [{ width: 30 }, { height: 0 }, { properties: { period: 45 } }] }).length).toBe(3);
  });
});

describe("a range a widget keeps", () => {
  test("is read from start, and written as whole hours or days", async () => {
    const { ownRange, startOf } = await import("./dashboard-model");
    expect(ownRange({ properties: { start: "-PT3H" } }, 10_000_000)).toEqual({ from: 10_000_000 - 3 * 3_600_000, to: 10_000_000 });
    expect(ownRange({ properties: {} }, 1)).toBeNull();
    expect(startOf(3 * 3_600_000)).toBe("-PT3H");
    expect(startOf(86_400_000)).toBe("-P1D");
  });
});
