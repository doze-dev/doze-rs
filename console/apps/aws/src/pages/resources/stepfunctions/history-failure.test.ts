import { describe, expect, test } from "bun:test";
import { firstFailure, type HistoryEvent, statesOfEvents } from "./history";

const run: HistoryEvent[] = [
  { id: 1, timestamp: 1, type: "ExecutionStarted" },
  { id: 2, timestamp: 2, type: "TaskStateEntered", stateEnteredEventDetails: { name: "NotifyCustomer" } },
  { id: 3, timestamp: 3, type: "TaskScheduled" },
  { id: 4, timestamp: 4, type: "TaskFailed", taskFailedEventDetails: { error: "KeyError" } },
  { id: 5, timestamp: 5, type: "ExecutionFailed", executionFailedEventDetails: { error: "KeyError" } },
];

describe("the event the event view opens first", () => {
  test("is the first failure inside a state, not the execution's own", () => {
    expect(firstFailure(run)?.id).toBe(4);
  });

  test("is the execution's failure when no state reports one, and none when nothing failed", () => {
    expect(firstFailure([run[0]!, run[4]!])?.id).toBe(5);
    expect(firstFailure(run.slice(0, 3))).toBeNull();
  });

  test("names the state each event happened in", () => {
    const of = statesOfEvents(run);
    expect(of.get(4)).toBe("NotifyCustomer");
    expect(of.get(5)).toBeUndefined();
  });
});
