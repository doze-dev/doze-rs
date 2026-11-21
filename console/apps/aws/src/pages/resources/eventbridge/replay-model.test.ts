import { describe, expect, test } from "bun:test";
import { emptyReplay, nameProblem, replayProblem, startReplayInput, windowProblem } from "./replay-model";

describe("the start-replay form", () => {
  const f = { ...emptyReplay("payments-90d", Date.UTC(2026, 10, 8, 8, 13, 41)), name: "payments-redo" };
  test("the window is the last hour, to the second", () => {
    expect(new Date(f.endMs!).toISOString()).toBe("2026-11-08T08:13:41.000Z");
    expect(new Date(f.startMs!).toISOString()).toBe("2026-11-08T07:13:41.000Z");
  });
  test("its problems", () => {
    expect(nameProblem("")).toBe("Name the replay.");
    expect(windowProblem(2, 1)).toBe("The end must be after the start.");
    expect(windowProblem(null, 1)).toContain("Set the start");
    expect(replayProblem(f, [])).toBeNull();
    expect(replayProblem(f, ["payments-redo"])).toBe("payments-redo already ran.");
    expect(replayProblem({ ...f, archive: null }, [])).toBe("Choose the archive to replay.");
    expect(replayProblem({ ...f, all: false }, [])).toContain("at least one rule");
  });
  test("StartReplay names the archive, the window in seconds and the bus; rules only when picked", () => {
    const arn = (r: string) => `arn:rule/${r}`;
    expect(startReplayInput(f, "arn:archive", "arn:bus", arn)).toEqual({ ReplayName: "payments-redo", EventSourceArn: "arn:archive", EventStartTime: f.startMs! / 1000, EventEndTime: f.endMs! / 1000, Destination: { Arn: "arn:bus" } });
    expect(startReplayInput({ ...f, all: false, rules: ["a"], description: "d" }, "arn:archive", "arn:bus", arn)).toMatchObject({ Description: "d", Destination: { Arn: "arn:bus", FilterArns: ["arn:rule/a"] } });
  });
});
