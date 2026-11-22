import { describe, expect, test } from "bun:test";
import { MOST_PINNED, parse, placeOf, toFront, togglePin } from "./pins";

const p = (name: string) => ({ service: "sqs", kind: "queues", name });

describe("placeOf", () => {
  test("is the resource a path is the page of", () => {
    expect(placeOf("/resources/sqs/queues/pick-lists")).toEqual(p("pick-lists"));
    expect(placeOf("/resources/s3/buckets/a%20b")).toEqual({ service: "s3", kind: "buckets", name: "a b" });
  });
  test("is nothing for a list, a service or another space", () => {
    expect(placeOf("/resources/sqs/queues")).toBeNull();
    expect(placeOf("/resources/sqs")).toBeNull();
    expect(placeOf("/live")).toBeNull();
    expect(placeOf("/resources/sqs/queues/a/b")).toBeNull();
  });
});

describe("the lists", () => {
  test("parse keeps well-formed entries once", () => {
    expect(parse(JSON.stringify([p("a"), p("a"), { service: 1 }, p("b")]))).toEqual([p("a"), p("b")]);
    expect(parse("not json")).toEqual([]);
    expect(parse(null)).toEqual([]);
  });
  test("a place opened again moves to the front, and the list is cut", () => {
    expect(toFront([p("a"), p("b"), p("c")], p("c"), 3).map((x) => x.name)).toEqual(["c", "a", "b"]);
    expect(toFront([p("a"), p("b")], p("z"), 2).map((x) => x.name)).toEqual(["z", "a"]);
  });
  test("pinning twice unpins, and the pins are bounded", () => {
    expect(togglePin([p("a")], p("a"))).toEqual([]);
    let l: ReturnType<typeof parse> = [];
    for (let i = 0; i < MOST_PINNED + 3; i++) l = togglePin(l, p(String(i)));
    expect(l.length).toBe(MOST_PINNED);
    expect(l[l.length - 1]?.name).toBe(String(MOST_PINNED + 2));
  });
});

import { deletedBy } from "./pins";
test("a request to delete a resource names it", () => {
  expect(deletedBy("/api/v1/resources/sqs/queues/pick-lists?clear=yes")).toEqual(p("pick-lists"));
  expect(deletedBy("/api/v1/resources/sqs/queues")).toBeNull();
  expect(deletedBy("/api/v1/resources/sqs/queues/q/tags")).toBeNull();
});
