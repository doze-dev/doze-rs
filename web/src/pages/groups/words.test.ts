import { describe, expect, test } from "bun:test";
import { agoNear, holdsPhrase, lasted, memberLabel, partList, partsPhrase, seconds, span, spanPrecise, stamp } from "./words";
import { groupsSearch, groupWireSearch, lagSearch, storySearch } from "./search";

describe("the Groups words say what the classic console says", () => {
  test("spans and timeouts", () => {
    expect(span(41_000)).toBe("41 s");
    expect(span(125_000)).toBe("2 min");
    expect(span(3 * 3600_000 + 5 * 60_000)).toBe("3 h 5 min");
    expect(span(3 * 86_400_000 + 4 * 3600_000)).toBe("3 d 4 h");
    expect(spanPrecise(2_100)).toBe("2.1 s");
    expect(spanPrecise(45_000)).toBe("45 s");
    expect(seconds(45_000)).toBe("45 s");
    expect(seconds(60_000)).toBe("60 s");
    expect(seconds(180_000)).toBe("3 min");
    expect(lasted(1_100)).toBe("1.1 s");
    expect(lasted(317_000)).toBe("5 m 17 s");
    expect(lasted(2 * 3600_000 + 24 * 60_000)).toBe("2 h 24 m");
    expect(agoNear(2_900)).toBe("3 s ago");
  });
  test("a stamp is the clock today and the date otherwise", () => {
    const now = Date.UTC(2026, 10, 13, 12, 0, 0);
    expect(stamp(Date.UTC(2026, 10, 13, 9, 5, 7), now)).toBe("09:05:07");
    expect(stamp(Date.UTC(2026, 10, 6, 0, 14), now)).toBe("Nov 6 00:14");
  });
  test("member ids lose the broker's UUID", () => {
    expect(memberLabel("fulfilment-py-1-7b0f6a0e-1111-2222-3333-444455556666")).toBe("fulfilment-py-1");
    expect(memberLabel("search-indexer-1")).toBe("search-indexer-1");
  });
  test("partitions fold into runs", () => {
    const ps = [0, 1, 2, 3, 4, 5].map((partition) => ({ topic: "clickstream", partition }));
    expect(partList([...ps, { topic: "inventory.levels", partition: 0 }])).toBe("clickstream-0 to 5, inventory.levels-0");
    expect(partList([{ topic: "orders", partition: 0 }, { topic: "orders", partition: 1 }])).toBe("orders-0, orders-1");
    expect(partsPhrase([["payments.settled", [0, 1]], ["pick-lists", [0]]])).toBe("payments.settled-0, -1 and pick-lists-0");
    expect(holdsPhrase("py-1", [["clickstream", [0, 1, 2, 3, 4, 5]]], true, true)).toBe("It holds clickstream-0 to 5");
  });
});

describe("the Groups addresses keep only what they know", () => {
  test("states are the three words, in the list's order, once each", () => {
    expect(groupsSearch({ q: "pay", state: ["empty", "stable", "stable", "nonsense"] })).toEqual({ q: "pay", state: ["stable", "empty"] });
    expect(groupsSearch({ state: "rebalancing" })).toEqual({ q: undefined, state: ["rebalancing"] });
    expect(groupsSearch({})).toEqual({ q: undefined, state: undefined });
  });
  test("the tabs' own params", () => {
    expect(lagSearch({ expiring: "1" })).toEqual({ expiring: 1 });
    expect(lagSearch({ expiring: "yes" })).toEqual({ expiring: undefined });
    expect(storySearch({ generation: "31", rule: 1 })).toEqual({ generation: 31, rule: 1 });
    expect(storySearch({ generation: "x" })).toEqual({ generation: undefined, rule: undefined });
    expect(groupWireSearch({ kind: "join", refused: "1", q: "py" })).toEqual({ kind: "join", refused: 1, q: "py" });
    expect(groupWireSearch({ kind: "everything" })).toEqual({ kind: undefined, refused: undefined, q: undefined });
  });
});
