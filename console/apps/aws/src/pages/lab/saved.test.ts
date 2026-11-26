import { describe, expect, test } from "bun:test";
import { MOST_NAME, MOST_SAVED, parseSaved, type Saved, saveInput } from "./saved";

const item = (name: string, at = 1): Saved => ({ id: `id-${name}-${at}`, name, at, target: "sqs", service: "sqs", title: "SQS", op: "SendMessage", input: { QueueUrl: "q", MessageBody: "m" } });

describe("saved inputs", () => {
  test("a saved input goes first, and the same name replaces it (case does not tell two names apart)", () => {
    const one = saveInput([], item("orders ping"));
    expect(one.ok).toBe(true);
    if (!one.ok) return;
    const two = saveInput(one.list, item("Another"));
    const three = saveInput(two.ok ? two.list : [], item("ORDERS PING", 2));
    expect(three.ok && three.list.map((s) => s.name)).toEqual(["ORDERS PING", "Another"]);
  });

  test("a name is needed, is cut at its limit and has no control character", () => {
    expect(saveInput([], item("  ")).ok).toBe(false);
    expect(saveInput([], item("x".repeat(MOST_NAME + 1)))).toEqual({ ok: false, why: `A name is at most ${MOST_NAME} characters.` });
    expect(saveInput([], item("a\u0007b")).ok).toBe(false);
    expect(saveInput([], item(" kept  ")).ok).toBe(true);
  });

  test("thirty are kept, and a thirty-first is refused in words (replacing one is not a new one)", () => {
    let list: Saved[] = [];
    for (let i = 0; i < MOST_SAVED; i++) {
      const r = saveInput(list, item(`n${i}`));
      expect(r.ok).toBe(true);
      list = r.ok ? r.list : list;
    }
    expect(saveInput(list, item("one more"))).toEqual({ ok: false, why: "30 inputs are kept. Delete one first." });
    const again = saveInput(list, item("n3", 9));
    expect(again.ok && again.list).toHaveLength(MOST_SAVED);
  });

  test("an entry is read back whole or not at all", () => {
    const s = item("a");
    expect(parseSaved(JSON.parse(JSON.stringify(s)))).toEqual(s);
    expect(parseSaved({ ...s, input: [] })).toBeNull();
    expect(parseSaved({ ...s, name: "" })).toBeNull();
    expect(parseSaved(null)).toBeNull();
  });
});
