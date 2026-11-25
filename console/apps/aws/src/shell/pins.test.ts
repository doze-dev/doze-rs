import { describe, expect, test } from "bun:test";
import { isGone, keyed, labelOf, MOST_PINNED, parse, placeOf, toFront, togglePin } from "./pins";

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

describe("labelOf", () => {
  test("is the name, and for a policy addressed by its ARN the policy's name", () => {
    expect(labelOf(p("pick-lists"))).toBe("pick-lists");
    expect(labelOf({ service: "iam", kind: "policies", name: "arn:aws:iam::000000000000:policy/service-role/report-job-readonly" })).toBe("report-job-readonly");
    expect(labelOf({ service: "iam", kind: "policies", name: "arn:aws:iam::aws:policy/PowerUserAccess" })).toBe("PowerUserAccess");
    expect(labelOf({ service: "iam", kind: "users", name: "dev" })).toBe("dev");
  });
});

describe("a deleted resource leaves the rail", () => {
  const cw = (name: string) => ({ service: "cloudwatch", kind: "alarms", name });
  test("a place is gone by its name, or by the tail of its ARN, in its own service only", () => {
    expect(isGone(cw("larder-fulfilment-lag"), "cloudwatch", ["larder-fulfilment-lag"])).toBe(true);
    expect(isGone(cw("other"), "cloudwatch", ["larder-fulfilment-lag"])).toBe(false);
    expect(isGone(p("larder-fulfilment-lag"), "cloudwatch", ["larder-fulfilment-lag"])).toBe(false);
    expect(isGone({ service: "iam", kind: "policies", name: "arn:aws:iam::1:policy/team/readers" }, "iam", ["readers"])).toBe(true);
    expect(isGone(cw("a"), "cloudwatch", [""])).toBe(false);
  });
});

describe("keyed", () => {
  test("the default region keeps the key it always had, another region has its own", () => {
    expect(keyed("doze-aws.pinned", null)).toBe("doze-aws.pinned");
    expect(keyed("doze-aws.pinned", "eu-west-1")).toBe("doze-aws.pinned@eu-west-1");
    expect(keyed("doze-aws.visited", "eu-west-1")).not.toBe(keyed("doze-aws.visited", "us-west-2"));
  });
});
