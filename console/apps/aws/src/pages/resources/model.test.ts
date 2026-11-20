import { describe, expect, test } from "bun:test";
import type { ResourcesIndex } from "../../api/types/ResourcesIndex";
import { resolve, seconds, shown, sortKey, timeMs } from "./model";

const kind = (id: string, members: string[]) => ({ id, title: id, one: id, members, count: null, more: false, names: [], error: null });
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
