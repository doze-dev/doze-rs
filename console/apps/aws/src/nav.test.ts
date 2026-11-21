import { expect, test } from "bun:test";
import { isActive, SPACES } from "./nav";

test("the rail has the six spaces of the design, in its order", () => {
  expect(SPACES.map((s) => s.label)).toEqual(["Home", "Live", "Resources", "Access", "Lab", "Settings"]);
  expect(new Set(SPACES.map((s) => s.to)).size).toBe(6);
});

test("a space is active on its own address and under it; Home only at the root", () => {
  const at = (path: string) => SPACES.filter((s) => isActive(s, path)).map((s) => s.label);
  expect(at("/")).toEqual(["Home"]);
  expect(at("/live")).toEqual(["Live"]);
  expect(at("/resources/sqs/queues")).toEqual(["Resources"]);
  // A name that only starts like a space is not in it.
  expect(at("/labs")).toEqual([]);
  expect(at("/nowhere")).toEqual([]);
});
