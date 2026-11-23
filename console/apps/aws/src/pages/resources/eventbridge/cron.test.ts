import { describe, expect, test } from "bun:test";
import { cronProblem, cronWords, nextRuns, parseCron, parseRate, runWords, values } from "./cron";

const f = (e: string) => parseCron(e)!;

describe("a cron expression", () => {
  test("has six fields", () => {
    expect(parseCron("cron(0 18 ? * MON-FRI *)")).toEqual({ minutes: "0", hours: "18", dom: "?", month: "*", dow: "MON-FRI", year: "*" });
    expect(parseCron("cron(0 18 ? * MON-FRI)")).toBeNull();
    expect(parseCron("rate(5 minutes)")).toBeNull();
  });
  test("its fields name values", () => {
    expect(values("MON-FRI", "dow")).toEqual([2, 3, 4, 5, 6]);
    expect(values("*/15", "minutes")).toEqual([0, 15, 30, 45]);
    expect(values("1,15", "dom")).toEqual([1, 15]);
    expect(values("10/20", "minutes")).toEqual([10, 30, 50]);
    expect(values("99", "minutes")).toBeNull();
    expect(values("5L", "dom")).toBeNull();
  });
  test("day of month and day of week can't both have a value", () => {
    expect(cronProblem(f("cron(0 18 1 * MON *)"))).toContain("can't both");
    expect(cronProblem(f("cron(0 18 ? * * *)"))).toBeNull();
    expect(cronProblem(f("cron(0 18 * * * *)"))).toContain("can't both");
  });
});

describe("its next runs", () => {
  const from = Date.UTC(2026, 10, 8, 8, 13); // Sunday Nov 8 2026
  test("weekdays at 18:00 UTC start on the Monday", () => {
    const runs = nextRuns(f("cron(0 18 ? * MON-FRI *)"), from, 6)!;
    expect(runs.map(runWords)).toEqual(["Mon, Nov 9 18:00 UTC", "Tue, Nov 10 18:00 UTC", "Wed, Nov 11 18:00 UTC", "Thu, Nov 12 18:00 UTC", "Fri, Nov 13 18:00 UTC", "Mon, Nov 16 18:00 UTC"]);
  });
  test("every 15 minutes, and a day of the month", () => {
    expect(nextRuns(f("cron(*/15 * * * ? *)"), from, 3)!.map((t) => new Date(t).toISOString())).toEqual(["2026-11-08T08:15:00.000Z", "2026-11-08T08:30:00.000Z", "2026-11-08T08:45:00.000Z"]);
    expect(nextRuns(f("cron(0 2 1 * ? *)"), from, 2)!.map((t) => new Date(t).toISOString())).toEqual(["2026-12-01T02:00:00.000Z", "2027-01-01T02:00:00.000Z"]);
  });
  test("what is not worked out is null; a date that never comes ends", () => {
    expect(nextRuns(f("cron(0 2 L * ? *)"), from)).toBeNull();
    expect(nextRuns(f("cron(0 2 31 2 ? *)"), from)).toEqual([]);
  });
});

describe("its reading", () => {
  test("a time and the days", () => {
    expect(cronWords(f("cron(0 18 ? * MON-FRI *)"))).toBe("6:00 pm UTC every Monday through Friday");
    expect(cronWords(f("cron(30 2 * * ? *)"))).toBe("2:30 am UTC every day");
    expect(cronWords(f("cron(0 12 15 * ? *)"))).toBe("12:00 pm UTC on day 15 of the month");
    expect(cronWords(f("cron(*/10 * * * ? *)"))).toBe("every 10 minutes");
  });
});

describe("a rate expression", () => {
  test("is a positive number and a unit, singular for 1", () => {
    expect(parseRate("rate(5 minutes)")).toEqual({ rate: { value: 5, unit: "minute" } });
    expect(parseRate("rate(1 hour)")).toEqual({ rate: { value: 1, unit: "hour" } });
    expect(parseRate("rate(1 hours)")).toEqual({ problem: "The unit is singular for 1: rate(1 hour)." });
    expect(parseRate("rate(5 minute)")).toEqual({ problem: "The unit is plural above 1: rate(5 minutes)." });
    expect(parseRate("rate(0 minutes)")).toEqual({ problem: "The value of a rate is a positive whole number." });
    expect(parseRate("rate(5 weeks)")).toEqual({ problem: "The unit of a rate is minute, hour or day." });
  });
});
