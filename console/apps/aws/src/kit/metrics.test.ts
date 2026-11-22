import { describe, expect, test } from "bun:test";
import { metricInput, periodFor, sharedUnit, toSeries, unitInput } from "./metrics";

const q = { id: "m0", label: "OrdersPlaced", namespace: "Larder/Orders", metric: "OrdersPlaced", dimensions: { Environment: "prod" } };

describe("metric data", () => {
  test("the period is a multiple of 60 s that gives about 60 points", () => {
    expect(periodFor(3 * 3_600_000)).toBe(180);
    expect(periodFor(60_000)).toBe(60);
    expect(periodFor(7 * 86_400_000) % 60).toBe(0);
  });
  test("the input is the service's own", () => {
    const i = metricInput([q], { from: 1_000_000, to: 4_600_000 });
    expect(i.StartTime).toBe(1000);
    expect(i.EndTime).toBe(4600);
    const first = i.MetricDataQueries[0] as { MetricStat: { Metric: { Dimensions: unknown }; Stat: string } };
    expect(first.MetricStat.Metric.Dimensions).toEqual([{ Name: "Environment", Value: "prod" }]);
    expect(first.MetricStat.Stat).toBe("Sum");
  });
  test("math is an expression over the other queries, and a hidden query is asked for but not drawn", () => {
    const queries = [{ ...q, hide: true }, { id: "e1", label: "Doubled", namespace: "", metric: "", expression: "m0*2" }];
    expect(metricInput(queries, { from: 0, to: 3_600_000 }).MetricDataQueries).toMatchObject([{ Id: "m0", ReturnData: false }, { Id: "e1", Expression: "m0*2", ReturnData: true }]);
    expect(toSeries({ MetricDataResults: [{ Id: "e1", Timestamps: [1], Values: [2] }] }, queries).map((x) => x.id)).toEqual(["e1"]);
  });
  test("the answer becomes a series per query, in ms, sorted; a missing query is an empty series", () => {
    const s = toSeries({ MetricDataResults: [{ Id: "m0", Label: "OrdersPlaced", Timestamps: [1700000060, 1700000000], Values: [2, 1] }] }, [q, { ...q, id: "m1", label: "Other" }]);
    expect(s[0]!.points).toEqual([{ t: 1700000000000, v: 1 }, { t: 1700000060000, v: 2 }]);
    expect(s[1]).toEqual({ id: "m1", label: "Other", points: [] });
    expect(toSeries(undefined, [q])[0]!.points).toEqual([]);
  });
  test("ISO timestamps are read too", () => expect(toSeries({ MetricDataResults: [{ Id: "m0", Timestamps: ["2026-11-08T05:00:00Z"], Values: [4] }] }, [q])[0]!.points[0]).toEqual({ t: Date.UTC(2026, 10, 8, 5), v: 4 }));
});

describe("the unit of a metric", () => {
  test("is asked of GetMetricStatistics, which answers it with the data points", () => {
    const input = unitInput({ id: "q1", label: "L", namespace: "Larder/Orders", metric: "OrdersPlaced", dimensions: { Env: "prod" } }, { from: 3_600_000, to: 7_200_000 });
    expect(input).toEqual({ Namespace: "Larder/Orders", MetricName: "OrdersPlaced", Dimensions: [{ Name: "Env", Value: "prod" }], StartTime: 3600, EndTime: 7200, Period: 60, Statistics: ["SampleCount"] });
  });
  test("is shown only when every metric has the same one, and None is none", () => {
    expect(sharedUnit(["Count"])).toBe("Count");
    expect(sharedUnit(["Count", "Count"])).toBe("Count");
    expect(sharedUnit(["Count", "Milliseconds"])).toBeUndefined();
    expect(sharedUnit(["None"])).toBeUndefined();
    expect(sharedUnit(["Count", "None"])).toBeUndefined();
    expect(sharedUnit(["Count", undefined])).toBeUndefined();
    expect(sharedUnit([])).toBeUndefined();
  });
});
