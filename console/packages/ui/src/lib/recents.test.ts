import { expect, test } from "bun:test";
import { parseRecents, pushRecent, RECENTS_MOST } from "./recents";

const r = (to: string, kind = "page") => ({ kind, label: to.slice(1), to });

test("the newest is first, a place is kept once, and five at most", () => {
  let list = [r("/a"), r("/b")];
  list = pushRecent(list, r("/b"));
  expect(list.map((x) => x.to)).toEqual(["/b", "/a"]);
  for (const to of ["/c", "/d", "/e", "/f"]) list = pushRecent(list, r(to));
  expect(list).toHaveLength(RECENTS_MOST);
  expect(list[0]!.to).toBe("/f");
});

test("what is stored is not trusted", () => {
  expect(parseRecents(null)).toEqual([]);
  expect(parseRecents("not json")).toEqual([]);
  expect(parseRecents('{"kind":"page"}')).toEqual([]);
  const stored = JSON.stringify([r("/topics/orders", "topic"), { kind: "page", label: "x", to: "https://elsewhere.example/" }, { kind: "page", label: "y", to: "//elsewhere.example" }, { kind: 3, label: "z", to: "/z" }, { ...r("/ok"), extra: "dropped" }]);
  expect(parseRecents(stored)).toEqual([r("/topics/orders", "topic"), r("/ok")]);
});

test("an app may name the kinds it knows", () => {
  const stored = JSON.stringify([r("/topics/orders", "topic"), r("/queues/q", "queue")]);
  expect(parseRecents(stored, ["topic", "page"])).toEqual([r("/topics/orders", "topic")]);
});
