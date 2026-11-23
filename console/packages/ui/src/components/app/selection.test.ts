import { describe, expect, test } from "bun:test";
import { allState, prune, selectedWords, toggle, toggleAll } from "./selection";

describe("selection", () => {
  test("toggle adds and removes, leaving the old set alone", () => {
    const a = new Set(["a"]);
    expect([...toggle(a, "b")].sort()).toEqual(["a", "b"]);
    expect([...toggle(a, "a")]).toEqual([]);
    expect([...a]).toEqual(["a"]);
  });
  test("the header checkbox is none, some or all", () => {
    const ids = ["a", "b", "c"];
    expect(allState(new Set(), ids)).toBe("none");
    expect(allState(new Set(["a"]), ids)).toBe("some");
    expect(allState(new Set(ids), ids)).toBe("all");
    expect(allState(new Set(["z"]), ids)).toBe("none");
    expect(allState(new Set(), [])).toBe("none");
  });
  test("the header checkbox chooses all, then none", () => {
    const ids = ["a", "b"];
    const all = toggleAll(new Set(["a"]), ids);
    expect([...all].sort()).toEqual(["a", "b"]);
    expect(toggleAll(all, ids).size).toBe(0);
  });
  test("a row that is gone is no longer chosen", () => expect([...prune(new Set(["a", "gone"]), ["a", "b"])]).toEqual(["a"]));
  test("words", () => {
    expect(selectedWords(3)).toBe("3 selected");
    expect(selectedWords(1200)).toBe("1,200 selected");
    expect(selectedWords(2, "policy")).toBe("2 policies selected");
    expect(selectedWords(1, "policy")).toBe("1 policy selected");
    expect(selectedWords(1, "alarm")).toBe("1 alarm selected");
    expect(selectedWords(2, "alarm")).toBe("2 alarms selected");
  });
});
