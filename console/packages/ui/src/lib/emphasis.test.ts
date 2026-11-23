import { expect, test } from "bun:test";
import { emphasise } from "./emphasis";

test("a name is set apart whole, never inside a longer name", () => {
  expect(emphasise("pick-lists-dlq is the dead-letter queue of pick-lists.", { names: ["pick-lists", "pick-lists-dlq"] })).toEqual([
    { text: "pick-lists-dlq", kind: "name" },
    { text: " is the dead-letter queue of ", kind: null },
    { text: "pick-lists", kind: "name" },
    { text: ".", kind: null },
  ]);
  // The one name alone does not reach into the other.
  expect(emphasise("It moves to pick-lists-dlq.", { names: ["pick-lists"] })).toEqual([{ text: "It moves to pick-lists-dlq.", kind: null }]);
  // A full stop ends a name; a dot inside one does not.
  expect(emphasise("Delete orders.fifo.", { names: ["orders", "orders.fifo"] }).filter((p) => p.kind).map((p) => p.text)).toEqual(["orders.fifo"]);
});

test("figures are set apart as bold, by text or by pattern", () => {
  const parts = emphasise("7 queues hold 58 messages waiting and 6 in flight.", { bold: [/\d+(?= messages| in flight)/] });
  expect(parts.filter((p) => p.kind === "bold").map((p) => p.text)).toEqual(["58", "6"]);
  expect(emphasise("It has 1 message.", { bold: ["1 message"] })[1]).toEqual({ text: "1 message", kind: "bold" });
});

test("no target, an empty one or a sentence without it is one plain part", () => {
  expect(emphasise("Empty.", {})).toEqual([{ text: "Empty.", kind: null }]);
  expect(emphasise("Empty.", { names: [""], bold: [""] })).toEqual([{ text: "Empty.", kind: null }]);
  expect(emphasise("", { names: ["x"] })).toEqual([{ text: "", kind: null }]);
});

test("overlapping targets: the one that starts first wins, then the longer", () => {
  const p = emphasise("pick-lists-dlq here", { names: ["pick-lists-dlq"], bold: ["pick-lists-dlq here"] });
  expect(p).toEqual([{ text: "pick-lists-dlq here", kind: "bold" }]);
});
