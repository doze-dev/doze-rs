import { describe, expect, test } from "bun:test";
import { blockerWords, groupTone, singular } from "./blockers";
import type { Blocker } from "./contract";

const g = (n: number, over: Partial<Blocker> = {}): Blocker => ({ title: "Attached policies", items: Array.from({ length: n }, (_, i) => ({ name: `p${i}`, link: null })), clears: false, blocks: false, more: false, ...over });

describe("blockerWords", () => {
  test("what the delete clears goes with it", () => {
    expect(blockerWords(g(1, { clears: true, blocks: true }))).toBe("1 attached policy is cleared with it");
    expect(blockerWords(g(2, { clears: true }))).toBe("2 attached policies are cleared with it");
  });
  test("what blocks it must be removed first", () => expect(blockerWords(g(2, { title: "Objects", blocks: true }))).toBe("2 objects must be removed first"));
  test("what is only left behind says so", () => {
    expect(blockerWords(g(1, { title: "Event source mappings" }))).toBe("1 event source mapping stays behind");
    expect(blockerWords(g(3, { title: "Event source mappings" }))).toBe("3 event source mappings stay behind");
  });
});

describe("groupTone", () => {
  test("a warning, a stop, a note", () => {
    expect(groupTone(g(1, { clears: true, blocks: true }))).toBe("warn");
    expect(groupTone(g(1, { blocks: true }))).toBe("bad");
    expect(groupTone(g(1))).toBe("info");
  });
});

describe("singular", () => {
  test("the last word, for a count of one", () => {
    expect(singular("Attached policies")).toBe("attached policy");
    expect(singular("Objects")).toBe("object");
    expect(singular("Access")).toBe("access");
    expect(singular("Event source mappings")).toBe("event source mapping");
  });
});
