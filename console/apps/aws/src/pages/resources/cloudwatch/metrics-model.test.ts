import { describe, expect, test } from "bun:test";
import type { MetricRef } from "./api";
import { groupMetrics, keyOf, metricsIn } from "./metrics-model";

const m = (Namespace: string, MetricName: string, d: Record<string, string> = {}): MetricRef => ({ Namespace, MetricName, Dimensions: Object.entries(d).map(([Name, Value]) => ({ Name, Value })) });
const all = [m("Larder/Orders", "OrdersPlaced", { Environment: "prod" }), m("Larder/Orders", "OrdersFailed", { Environment: "prod" }), m("Larder/Queues", "DlqMessages", { QueueName: "pick-lists-dlq" }), m("AWS/Lambda", "Errors", { FunctionName: "send-email" }), m("AWS/Lambda", "Errors", { FunctionName: "resize" }), m("AWS/Lambda", "Invocations", { FunctionName: "resize" })];

describe("browsing metrics", () => {
  test("custom and AWS namespaces, one row for each set of dimension names, counting the metrics", () => {
    const g = groupMetrics(all, "");
    expect(g.custom.map((r) => [r.namespace, r.key, r.count])).toEqual([["Larder/Orders", "Environment", 2], ["Larder/Queues", "QueueName", 1]]);
    expect(g.aws.map((r) => [r.namespace, r.key, r.count])).toEqual([["AWS/Lambda", "FunctionName", 2]]);
  });
  test("the search reads the metric, the dimension names and values, and the namespace", () => {
    expect(groupMetrics(all, "pick-lists").custom.map((r) => r.namespace)).toEqual(["Larder/Queues"]);
    expect(groupMetrics(all, "nothing at all")).toEqual({ custom: [], aws: [] });
    expect(groupMetrics(all, "OrdersFailed").custom[0]?.count).toBe(1);
  });
  test("a row opens onto its metrics", () => {
    expect(metricsIn(all, "AWS/Lambda", "FunctionName", "").length).toBe(3);
    expect(metricsIn(all, "AWS/Lambda", "FunctionName", "send-email").map((x) => x.MetricName)).toEqual(["Errors"]);
    expect(keyOf(["a", "b"])).toBe("a,b");
  });
});
