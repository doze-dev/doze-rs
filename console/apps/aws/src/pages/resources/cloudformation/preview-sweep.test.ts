import { describe, expect, test } from "bun:test";
import { beat, LIVE_MS, readBeats, stalePreviews, unbeat } from "./preview-sweep";

const memory = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};

describe("preview change sets nobody is using", () => {
  const now = 1_000_000;
  test("only the wizard's own names, and not one beating or kept", () => {
    const names = ["first-look", "console-preview-1", "console-preview-2", "console-preview-3", "console-preview-x"];
    expect(stalePreviews(names, now, "console-preview-2", { "console-preview-3": now - 5_000 })).toEqual(["console-preview-1"]);
  });
  test("a beat older than the live window no longer protects it", () => {
    expect(stalePreviews(["console-preview-1"], now, null, { "console-preview-1": now - LIVE_MS - 1 })).toEqual(["console-preview-1"]);
  });
  test("a review step beats while open and is forgotten when it closes", () => {
    const s = memory();
    beat("console-preview-1", now, s);
    expect(stalePreviews(["console-preview-1"], now + 1_000, null, readBeats(s))).toEqual([]);
    unbeat("console-preview-1", s);
    expect(stalePreviews(["console-preview-1"], now + 1_000, null, readBeats(s))).toEqual(["console-preview-1"]);
  });
  test("a storage that says nothing sensible is no beats", () => {
    const s = { getItem: () => "[1,2]", setItem: () => undefined };
    expect(readBeats(s)).toEqual({});
  });
});
