import { expect, test } from "bun:test";
import { channelY, CARD, GAP, lift, metrics, MIN_CARD, MIN_GAP, overRoute, roundedPath, skips } from "./map-geometry";

test("a map that fits keeps its sizes and is spread over the box", () => {
  expect(metrics(3, 1100)).toEqual({ card: CARD, gap: GAP, width: 1100, scrolls: false });
});

test("five columns in the 1,080 px a 1440 px window leaves shrink to fit and do not scroll", () => {
  const m = metrics(5, 1080);
  expect(m.scrolls).toBe(false);
  expect(m.width).toBe(1080);
  expect(m.card).toBeGreaterThanOrEqual(MIN_CARD);
  expect(m.gap).toBeGreaterThanOrEqual(MIN_GAP);
  expect(16 + 5 * m.card + 4 * m.gap).toBeLessThanOrEqual(1080);
});

test("past the least sizes the map is as wide as they need, and scrolls", () => {
  const m = metrics(5, 500);
  expect(m).toEqual({ card: MIN_CARD, gap: MIN_GAP, width: 16 + 5 * MIN_CARD + 4 * MIN_GAP, scrolls: true });
  expect(metrics(1, 100).scrolls).toBe(true);
});

test("a line skips a column when it runs over one", () => {
  expect(skips({ col: 0 }, { col: 1 })).toBe(false);
  expect(skips({ col: 0 }, { col: 2 })).toBe(true);
  // Only a line that goes right is routed over; one that goes back is drawn as any other.
  expect(skips({ col: 2 }, { col: 0 })).toBe(false);
});

test("the lines over the columns are kept apart, and the columns make room for them", () => {
  expect(lift(0)).toBe(0);
  expect(channelY(0)).toBeLessThan(channelY(1));
  expect(channelY(99)).toBe(channelY(9));
  for (const n of [1, 4, 10, 30]) expect(lift(n)).toBeGreaterThan(channelY(n - 1));
});

test("corners are rounded and a short run is not rounded past its middle", () => {
  expect(roundedPath([])).toBe("");
  expect(roundedPath([[0, 0], [100, 0], [100, 100]], 10)).toBe("M0 0L90 0Q100 0 100 10L100 100");
  expect(roundedPath([[0, 0], [4, 0], [4, 100]], 10)).toBe("M0 0L2 0Q4 0 4 2L4 100");
});

test("a line over a column goes out, up to its channel, along it, down, and in", () => {
  const r = overRoute(200, 300, 900, 250, 12, 300, 800, 0);
  expect(r.d.startsWith("M200 300")).toBe(true);
  expect(r.d.endsWith("L900 250")).toBe(true);
  expect(r.my).toBe(12);
  // Two lines in one gutter do not share their vertical.
  expect(overRoute(200, 300, 900, 250, 12, 300, 800, 1).d).not.toBe(r.d);
});
