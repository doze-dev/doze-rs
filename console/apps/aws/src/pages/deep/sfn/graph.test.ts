import { describe, expect, test } from "bun:test";
import { END, START, allBoxes, layout, type Definition } from "./graph";
import { readRun, type HistoryEvent } from "./history";

const CHOICE: Definition = {
  StartAt: "Load",
  States: {
    Load: { Type: "Task", Resource: "arn:aws:states:::dynamodb:getItem", Next: "Pick" },
    Pick: { Type: "Choice", Choices: [{ Variable: "$.big", BooleanEquals: true, Next: "Split" } as never], Default: "Small" },
    Small: { Type: "Pass", End: true },
    Split: {
      Type: "Parallel",
      Branches: [
        { StartAt: "A", States: { A: { Type: "Pass", End: true } } },
        { StartAt: "B1", States: { B1: { Type: "Wait", Next: "B2" }, B2: { Type: "Pass", End: true } } },
      ],
      Next: "Retry",
    },
    Retry: { Type: "Choice", Choices: [{ Variable: "$.again", Next: "Load" }], Default: "Done" },
    Done: { Type: "Succeed" },
  },
};

describe("a state machine's graph", () => {
  test("states sit one layer below the deepest state that leads to them, Start above and End below", () => {
    const g = layout(CHOICE);
    const y = (n: string) => g.boxes.find((b) => b.name === n)?.y ?? -1;
    expect(y("Start")).toBeLessThan(y("Load"));
    expect(y("Load")).toBeLessThan(y("Pick"));
    // Small and Split are both one below Pick: side by side.
    expect(y("Small")).toBe(y("Split") + (g.boxes.find((b) => b.name === "Split")!.h - 46) / 2);
    expect(y("Split")).toBeLessThan(y("Retry"));
    expect(y("Done")).toBeLessThan(y("End"));
    expect(g.boxes.find((b) => b.id === START)).toBeDefined();
    expect(g.boxes.find((b) => b.id === END)).toBeDefined();
  });

  test("a loop is a back edge drawn round the right side, and the layers do not count it", () => {
    const g = layout(CHOICE);
    const loop = g.edges.find((e) => e.from === "Retry" && e.to === "Load");
    expect(loop?.back).toBe(true);
    expect(loop?.label).toBe("$.again");
    expect(Math.max(...(loop?.points ?? []).map((p) => p[0]))).toBeGreaterThan(Math.max(...g.boxes.map((b) => b.x + b.w)));
    expect(g.edges.filter((e) => e.from === "Pick").map((e) => [e.to, e.kind])).toEqual([
      ["Split", "choice"],
      ["Small", "default"],
    ]);
  });

  test("a Parallel holds its branches side by side, each laid out inside it with ids of their own", () => {
    const g = layout(CHOICE);
    const split = g.boxes.find((b) => b.name === "Split")!;
    expect(split.inner.map((p) => p.title)).toEqual(["Branch 1", "Branch 2"]);
    const [a, b] = split.inner;
    expect(a.x + a.graph.w).toBeLessThanOrEqual(b.x);
    expect(split.w).toBeGreaterThanOrEqual(a.graph.w + b.graph.w);
    const ids = allBoxes(g).map((x) => x.id);
    expect(ids).toContain("Split/1/B2");
    expect(new Set(ids).size).toBe(ids.length);
  });

  test("a definition that is not one is an empty drawing, never a throw", () => {
    expect(layout({}).boxes.map((b) => b.name)).toEqual(["Start", "End"]);
    expect(layout({ StartAt: "X", States: { X: { Type: "Pass", Next: "Nowhere" } } }).edges.map((e) => e.to)).toEqual(["X"]);
  });
});

const ev = (id: number, type: string, more: Partial<HistoryEvent> = {}): HistoryEvent => ({ id, type, timestamp: id, previousEventId: id - 1, ...more });

describe("an execution's history", () => {
  test("each state is succeeded, running or failed, with what it took and gave, and the edges taken", () => {
    const events = [
      ev(1, "ExecutionStarted"),
      ev(2, "TaskStateEntered", { stateEnteredEventDetails: { name: "Load", input: "{}" } }),
      ev(3, "TaskStateExited", { stateExitedEventDetails: { name: "Load", output: '{"a":1}' } }),
      ev(4, "ChoiceStateEntered", { stateEnteredEventDetails: { name: "Pick", input: '{"a":1}' } }),
      ev(5, "ChoiceStateExited", { stateExitedEventDetails: { name: "Pick", output: '{"a":1}' } }),
      ev(6, "TaskStateEntered", { stateEnteredEventDetails: { name: "Small", input: '{"a":1}' } }),
      ev(7, "TaskFailed", { taskFailedEventDetails: { error: "Boom", cause: "it broke" } }),
      ev(8, "ExecutionFailed", { executionFailedEventDetails: { error: "Boom", cause: "it broke" } }),
    ];
    const run = readRun(events, "FAILED");
    expect(run.steps.get("Load")?.status).toBe("succeeded");
    expect(run.steps.get("Load")?.output).toBe('{"a":1}');
    expect(run.steps.get("Small")?.status).toBe("failed");
    expect(run.steps.get("Small")?.error).toBe("Boom");
    expect([...run.taken].sort()).toEqual(["Load>Pick", "Pick>Small", "__start>Load"]);
    expect(readRun(events.slice(0, 6), "RUNNING").steps.get("Small")?.status).toBe("running");
    expect(readRun(events.slice(0, 6), "ABORTED").steps.get("Small")?.status).toBe("cancelled");
  });
});

describe("an edge that passes a layer", () => {
  test("runs down a lane on the left, clear of the boxes it passes", () => {
    const g = layout({
      StartAt: "Route",
      States: { Route: { Type: "Choice", Choices: [{ Variable: "$.hold", Next: "Wait" }], Default: "Go" }, Wait: { Type: "Wait", Next: "Go" }, Go: { Type: "Pass", End: true } },
    });
    const skip = g.edges.find((e) => e.from === "Route" && e.to === "Go");
    const wait = g.boxes.find((b) => b.name === "Wait")!;
    expect(skip?.points.length).toBe(6);
    expect(Math.min(...(skip?.points ?? []).map((p) => p[0]))).toBeLessThan(wait.x);
    expect(g.edges.find((e) => e.from === "Route" && e.to === "Wait")?.points.length).toBe(2);
  });
});
