import { describe, expect, test } from "bun:test";
import type { KindCount } from "../../api/types/KindCount";
import { cardWords } from "./index-page";

const kind = (title: string, one: string, count: number | null, error: string | null = null): KindCount => ({ id: title.toLowerCase(), title, one, members: [], count, more: false, names: [], error, others: null });

describe("a service's card", () => {
  test("names the kinds there are some of", () => {
    expect(cardWords([kind("Rules", "rule", 1), kind("Event buses", "event bus", 1), kind("Archives", "archive", 0)])).toBe("1 rule · 1 event bus");
  });
  test("names its first kind with the zero when there is nothing of any", () => {
    expect(cardWords([kind("Topics", "topic", 0), kind("Subscriptions", "subscription", 0)])).toBe("0 topics");
  });
  test("keeps a kind that could not be counted", () => {
    expect(cardWords([kind("Queues", "queue", 2), kind("Other", "other", null, "denied")])).toBe("2 queues · Other: not read");
  });
});
