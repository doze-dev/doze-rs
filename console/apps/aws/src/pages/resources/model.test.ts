import { describe, expect, test } from "bun:test";
import type { ResourcesIndex } from "../../api/types/ResourcesIndex";
import { columnLeast, columnSizes, isDocument, resolve, seconds, shown, sortKey, timeMs, titleOf } from "./model";

const kind = (id: string, members: string[]) => ({ id, title: id, one: id, members, count: null, more: false, names: [], error: null, others: null });
const INDEX: ResourcesIndex = {
  services: [
    { name: "sns", title: "SNS", group: "Messaging and events", served: true, described: true, note: null, kinds: [kind("topics", ["TopicArn", "TargetArn"]), kind("subscriptions", ["SubscriptionArn"])] },
    { name: "iam", title: "IAM", group: "Identity", served: true, described: false, note: "Not described yet", kinds: [] },
  ],
};

describe("the Resources pages", () => {
  test("an address is a kind's list, or a resource of the kind its member names, else of the first kind", () => {
    expect(resolve(INDEX, "sns", "subscriptions")).toEqual({ kind: "subscriptions", name: null });
    expect(resolve(INDEX, "sns", "orders")).toEqual({ kind: "topics", name: "orders" });
    expect(resolve(INDEX, "sns", "arn:aws:sns:r:a:orders:1", "SubscriptionArn")).toEqual({ kind: "subscriptions", name: "arn:aws:sns:r:a:orders:1" });
    expect(resolve(INDEX, "sns", "orders", "Nope")).toEqual({ kind: "topics", name: "orders" });
    expect(resolve(INDEX, "iam", "users")).toBe(null);
    expect(resolve(INDEX, "nope", "x")).toBe(null);
  });

  test("a value is written as its format asks, and as it is when it cannot be read", () => {
    expect(shown("12345", "number")).toBe("12,345");
    expect(shown("1536", "bytes")).toBe("1.5 KiB");
    expect(shown("1763700000", "time")).toBe("2025-11-21 04:40:00 UTC");
    expect(shown("1763700000000", "time")).toBe("2025-11-21 04:40:00 UTC");
    expect(shown("2025-11-21T04:40:00Z", "time")).toBe("2025-11-21 04:40:00 UTC");
    expect(shown("soon", "time")).toBe("soon");
    expect(shown("x", "number")).toBe("x");
    expect(seconds("345600")).toBe("4 days");
    expect(seconds("30")).toBe("30 seconds");
    expect(seconds("90")).toBe("90 seconds");
    expect(seconds("3600")).toBe("1 hour");
    expect(timeMs("1.5")).toBe(1500);
  });

  test("columns sort numbers as numbers and times by the moment", () => {
    expect(sortKey("10", "number")).toBe(10);
    expect(sortKey("2025-11-21T04:40:00Z", "time")).toBe(Date.UTC(2025, 10, 21, 4, 40));
    expect(sortKey("Orders", "text")).toBe("orders");
    expect(sortKey(null, "number")).toBe("");
  });
});

test("a page named by an ARN is titled by the name at its end", () => {
  expect(titleOf("arn:aws:iam::111122223333:policy/team/orders-send")).toBe("orders-send");
  expect(titleOf("orders")).toBe("orders");
});

test("a JSON field is a block only when it holds a document", () => {
  expect(isDocument('{"a":1}')).toBe(true);
  expect(isDocument("None")).toBe(false);
  expect(isDocument(null)).toBe(false);
});

test("a related list's longest values take the room left", () => {
  const sizes = columnSizes({
    columns: [
      { title: "Key", format: "mono" },
      { title: "Value", format: "mono" },
    ],
    rows: [{ cells: ["InboxUrl", "http://127.0.0.1:4566/111122223333/stack-inbox"] }],
  });
  expect(sizes[1]).toBeUndefined();
  expect(sizes[0]).toBe(110);
});

test("in a narrow window the column people read a row by keeps its values' width, a time its own", () => {
  const list = {
    columns: [
      { title: "Name", format: "mono" as const },
      { title: "Created", format: "time" as const },
    ],
    rows: [{ cells: ["d579d966-1700-4d1b-9e5a-5c1c3a3a0e11", "2026-11-20T23:41:38Z"] }],
  };
  const [name, created] = columnLeast(list);
  expect(name).toBe(190);
  expect(created).toBeCloseTo(229.8);
});
