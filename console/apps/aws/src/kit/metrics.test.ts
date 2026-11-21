import { describe, expect, test } from "bun:test";
import { metricInput, periodFor, toSeries } from "./metrics";

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
    expect(i.MetricDataQueries[0]!.MetricStat.Metric.Dimensions).toEqual([{ Name: "Environment", Value: "prod" }]);
    expect(i.MetricDataQueries[0]!.MetricStat.Stat).toBe("Sum");
  });
  test("the answer becomes a series per query, in ms, sorted; a missing query is an empty series", () => {
    const s = toSeries({ MetricDataResults: [{ Id: "m0", Label: "OrdersPlaced", Timestamps: [1700000060, 1700000000], Values: [2, 1] }] }, [q, { ...q, id: "m1", label: "Other" }]);
    expect(s[0]!.points).toEqual([{ t: 1700000000000, v: 1 }, { t: 1700000060000, v: 2 }]);
    expect(s[1]).toEqual({ id: "m1", label: "Other", points: [] });
    expect(toSeries(undefined, [q])[0]!.points).toEqual([]);
  });
  test("ISO timestamps are read too", () => expect(toSeries({ MetricDataResults: [{ Id: "m0", Timestamps: ["2026-11-08T05:00:00Z"], Values: [4] }] }, [q])[0]!.points[0]).toEqual({ t: Date.UTC(2026, 10, 8, 5), v: 4 }));
});
