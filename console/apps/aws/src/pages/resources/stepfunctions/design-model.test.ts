import { describe, expect, test } from "bun:test";
import type { Definition } from "./graph";
import { NO_HISTORY, changed, deleteState, insertOnEdge, moveState, offerOf, parseScope, redo, renameState, setState, undo, uniqueName } from "./design-model";

const line: Definition = { StartAt: "A", States: { A: { Type: "Pass", Next: "B" }, B: { Type: "Pass", End: true } } };
const wait = offerOf("wait")!;

describe("dropping a state on an edge", () => {
  test("between two states joins them through it", () => {
    const r = insertOnEdge(line, { scope: "", from: "A", to: "B", kind: "next", nth: 0 }, wait)!;
    expect(r.name).toBe("Wait");
    expect(r.def.States?.A?.Next).toBe("Wait");
    expect(r.def.States?.Wait).toEqual({ Type: "Wait", Seconds: 5, Next: "B" });
  });
  test("on the edge to End takes the place of the end", () => {
    const r = insertOnEdge(line, { scope: "", from: "B", to: null, kind: "end", nth: 0 }, wait)!;
    expect(r.def.States?.B).toEqual({ Type: "Pass", Next: "Wait" });
    expect(r.def.States?.Wait?.End).toBe(true);
  });
  test("on the edge from Start becomes the first state", () => {
    const r = insertOnEdge(line, { scope: "", from: null, to: "A", kind: "start", nth: 0 }, wait)!;
    expect(r.def.StartAt).toBe("Wait");
    expect(r.def.States?.Wait?.Next).toBe("A");
  });
  test("a state that ends a path goes only where the path ends", () => {
    const succeed = offerOf("succeed")!;
    expect(insertOnEdge(line, { scope: "", from: "A", to: "B", kind: "next", nth: 0 }, succeed)).toBeNull();
    const r = insertOnEdge(line, { scope: "", from: "B", to: null, kind: "end", nth: 0 }, succeed)!;
    expect(r.def.States?.B?.Next).toBe("Success");
    expect(r.def.States?.Success).toEqual({ Type: "Succeed" });
  });
  test("a choice keeps the way on as its default and gets a state for its rule", () => {
    const r = insertOnEdge(line, { scope: "", from: "A", to: "B", kind: "next", nth: 0 }, offerOf("choice")!)!;
    const c = r.def.States?.Choice;
    expect(c?.Default).toBe("B");
    expect(c?.Choices?.[0]?.Next).toBe("Pass");
    expect(r.def.States?.Pass).toEqual({ Type: "Pass", End: true });
  });
  test("inside a branch, the branch is changed and the rest is not", () => {
    const par = insertOnEdge(line, { scope: "", from: "A", to: "B", kind: "next", nth: 0 }, offerOf("parallel")!)!.def;
    const r = insertOnEdge(par, { scope: "Parallel/0/", from: "Pass", to: null, kind: "end", nth: 0 }, wait)!;
    expect(r.def.States?.Parallel?.Branches?.[0]?.States?.Pass).toEqual({ Type: "Pass", Next: "Wait" });
    expect(r.def.States?.Wait).toBeUndefined();
    expect(r.name).toBe("Wait");
  });
  test("a name is never reused", () => {
    expect(uniqueName(line, "A")).toBe("A (1)");
    expect(insertOnEdge(line, { scope: "", from: "A", to: "B", kind: "next", nth: 0 }, offerOf("pass")!)!.name).toBe("Pass");
  });
});

describe("deleting and renaming", () => {
  test("a deleted state is bypassed", () => {
    const d = deleteState({ StartAt: "A", States: { A: { Type: "Pass", Next: "B" }, B: { Type: "Pass", Next: "C" }, C: { Type: "Pass", End: true } } }, "B")!;
    expect(d.States?.A?.Next).toBe("C");
    expect(d.States?.B).toBeUndefined();
  });
  test("the last state ends the one before it", () => {
    expect(deleteState(line, "B")!.States?.A).toEqual({ Type: "Pass", End: true });
  });
  test("the first state hands Start on", () => {
    expect(deleteState(line, "A")!.StartAt).toBe("B");
  });
  test("a rename follows every reference", () => {
    const d = renameState({ StartAt: "A", States: { A: { Type: "Choice", Choices: [{ Next: "B" }], Default: "B" }, B: { Type: "Pass", End: true } } }, "B", "Done")!;
    expect(d.States?.A?.Default).toBe("Done");
    expect(d.States?.A?.Choices?.[0]?.Next).toBe("Done");
    expect(Object.keys(d.States ?? {})).toEqual(["A", "Done"]);
    expect(renameState(line, "B", "A")).toBeNull();
  });
  test("a state's body is replaced", () => {
    expect(setState(line, "B", { Type: "Succeed" })!.States?.B).toEqual({ Type: "Succeed" });
  });
});

test("a scope is read from the graph's ids", () => {
  expect(parseScope("Par/0/Inner/1/")).toEqual([{ state: "Par", index: 0 }, { state: "Inner", index: 1 }]);
  expect(parseScope("")).toEqual([]);
});

describe("moving a state", () => {
  const three: Definition = { StartAt: "A", States: { A: { Type: "Pass", Next: "B" }, B: { Type: "Wait", Seconds: 3, Next: "C" }, C: { Type: "Pass", End: true } } };
  test("takes it out of its place and puts it back on the edge, with its own body and name", () => {
    const moved = moveState(three, "A", { scope: "", from: "B", to: "C", kind: "next", nth: 0 })!;
    expect(moved.StartAt).toBe("B");
    expect(moved.States?.B?.Next).toBe("A");
    expect(moved.States?.A).toEqual({ Type: "Pass", Next: "C" });
    expect(moved.States?.C).toEqual({ Type: "Pass", End: true });
  });
  test("to the end of the path ends there", () => {
    const moved = moveState(three, "B", { scope: "", from: "C", to: null, kind: "end", nth: 0 })!;
    expect(moved.States?.A?.Next).toBe("C");
    expect(moved.States?.C).toEqual({ Type: "Pass", Next: "B" });
    expect(moved.States?.B).toEqual({ Type: "Wait", Seconds: 3, End: true });
  });
  test("an edge of its own place, and a Choice, do not move", () => {
    expect(moveState(three, "B", { scope: "", from: "A", to: "B", kind: "next", nth: 0 })).toBeNull();
    expect(moveState(three, "B", { scope: "", from: "B", to: "C", kind: "next", nth: 0 })).toBeNull();
    const choice: Definition = { StartAt: "C", States: { C: { Type: "Choice", Choices: [{ Variable: "$.x", StringEquals: "a", Next: "P" }], Default: "P" }, P: { Type: "Pass", End: true } } };
    expect(moveState(choice, "C", { scope: "", from: "P", to: null, kind: "end", nth: 0 })).toBeNull();
  });
});

describe("dropping a pattern", () => {
  test("brings the states that go together: a Lambda with Retry and Catch brings the Fail it catches to", () => {
    const r = insertOnEdge(line, { scope: "", from: "A", to: "B", kind: "next", nth: 0 }, offerOf("retry-catch")!)!;
    const task = r.def.States?.[r.name];
    expect(task?.Retry?.[0]?.MaxAttempts).toBe(3);
    expect(task?.Next).toBe("B");
    expect(task?.Catch?.[0]?.Next).toBe("Handle failure");
    expect(r.def.States?.["Handle failure"]?.Type).toBe("Fail");
  });
  test("a Parallel with two branches has two states of their own", () => {
    const r = insertOnEdge(line, { scope: "", from: "A", to: "B", kind: "next", nth: 0 }, offerOf("fan-out")!)!;
    const par = r.def.States?.Parallel;
    expect(par?.Branches).toHaveLength(2);
    expect(par?.Next).toBe("B");
  });
});

describe("undo and redo", () => {
  test("a change can be taken back and put back, and a new change forgets what was taken back", () => {
    let h = changed(NO_HISTORY, "one", "two");
    h = changed(h, "two", "three");
    const back = undo(h, "three")!;
    expect(back.text).toBe("two");
    const again = redo(back.history, "two")!;
    expect(again.text).toBe("three");
    expect(changed(back.history, "two", "four").future).toEqual([]);
    expect(undo(NO_HISTORY, "x")).toBeNull();
    expect(redo(NO_HISTORY, "x")).toBeNull();
  });
  test("a change that is none is not a step, and only the last 50 are kept", () => {
    expect(changed(NO_HISTORY, "same", "same")).toBe(NO_HISTORY);
    let h = NO_HISTORY;
    for (let i = 0; i < 60; i++) h = changed(h, String(i), String(i + 1));
    expect(h.past).toHaveLength(50);
    expect(h.past[0]).toBe("10");
  });
});
