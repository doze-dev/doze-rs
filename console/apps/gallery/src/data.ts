// The gallery's stand-in data: made here, the same on every load (no clock, no random), so a screenshot is comparable.
export type Resource = { id: string; name: string; kind: "queue" | "topic" | "table" | "bucket" | "function"; state: "Active" | "Idle" | "Failing"; calls: number; updated: number; note: string };

const KINDS = ["queue", "topic", "table", "bucket", "function"] as const;
const STEMS = ["orders", "payments", "pick-lists", "inventory", "shipments", "invoices", "returns", "clickstream", "audit", "notifications"];
const BASE = Date.UTC(2026, 10, 19, 9, 0, 0);

export function resources(n: number): Resource[] {
  return Array.from({ length: n }, (_, i) => {
    const kind = KINDS[i % KINDS.length]!;
    const stem = STEMS[(i * 7) % STEMS.length]!;
    const state = i % 23 === 5 ? "Failing" : i % 4 === 3 ? "Idle" : "Active";
    return {
      id: `r${i}`,
      name: i === 2 ? "a-resource-whose-name-is-far-longer-than-its-column-so-that-the-ellipsis-and-the-title-are-both-seen" : `${stem}-${String(i).padStart(4, "0")}`,
      kind,
      state,
      calls: (i * 7919) % 100_003,
      updated: BASE - i * 61_000,
      note: i % 9 === 0 ? "Dead letters go to pick-lists-dlq after 5 receives" : "",
    };
  });
}

export const LONG = "arn:aws:sqs:us-east-1:000000000000:a-queue-whose-name-is-very-long-indeed-and-goes-on-past-any-reasonable-column-width.fifo";

export const OPTIONS = STEMS.map((s, i) => ({ value: s, label: s, hint: `${(i + 1) * 3}` }));
