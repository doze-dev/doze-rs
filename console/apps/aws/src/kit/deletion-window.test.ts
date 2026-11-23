import { describe, expect, test } from "bun:test";
import { purgeDate, windowDays } from "./deletion-window";

describe("deletion window", () => {
  test("whole days inside the limits", () => {
    expect(windowDays("30")).toBe(30);
    expect(windowDays("7")).toBe(7);
    expect(windowDays(" 14 ")).toBe(14);
  });
  test("anything else is none", () => {
    for (const bad of ["6", "31", "", "abc", "7.5", "-8", "1e1"]) expect(windowDays(bad)).toBeNull();
    expect(windowDays("5", { min: 1, max: 9 })).toBe(5);
  });
  test("the day it ends, in UTC", () => expect(purgeDate(Date.UTC(2026, 10, 8, 9, 0), 7)).toBe("Nov 15, 2026"));
});
