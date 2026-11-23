import { describe, expect, test } from "bun:test";
import { canGoTo, nextStep, prevStep, stepStates, stepWords } from "./stepper-state";

describe("stepper state", () => {
  test("steps before the current are done, after are upcoming", () => {
    expect(stepStates(4, 2)).toEqual(["done", "done", "current", "upcoming"]);
    expect(stepStates(3, 0)).toEqual(["current", "upcoming", "upcoming"]);
    expect(stepStates(0, 0)).toEqual([]);
  });
  test("a person goes back freely and forward only as far as they have reached", () => {
    expect(canGoTo(0, 2, 2)).toBe(true);
    expect(canGoTo(2, 2, 2)).toBe(false);
    expect(canGoTo(3, 2, 2)).toBe(false);
    expect(canGoTo(3, 1, 3)).toBe(true);
    expect(canGoTo(-1, 1, 3)).toBe(false);
  });
  test("next and previous stay inside the steps", () => {
    expect(nextStep(3, 4)).toBe(3);
    expect(nextStep(1, 4)).toBe(2);
    expect(prevStep(0)).toBe(0);
    expect(prevStep(2)).toBe(1);
  });
  test("the words", () => expect(stepWords(3, 4)).toBe("Step 4 of 4"));
});
