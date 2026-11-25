import { describe, expect, test } from "bun:test";
import { onceLink, splitAddress } from "./once-link";

describe("where a forced event's result is", () => {
  test("each kind opens the page that shows what it did, with the name encoded", () => {
    expect(onceLink("expire-visibility", "orders.fifo")).toBe("/resources/sqs/queues/orders.fifo");
    expect(onceLink("fire-schedule", "default/nightly report")).toBe("/resources/scheduler/schedules/default%2Fnightly%20report");
    expect(onceLink("sweep-ttl", "sessions")).toBe("/resources/dynamodb/tables/sessions");
    expect(onceLink("cold-start", "resize")).toBe("/resources/lambda/functions/resize");
    expect(onceLink("rest-service", "sqs")).toBe("/live?console=shown&service=sqs");
    expect(onceLink("other", "x")).toBeNull();
  });

  test("an address is split for a router link", () => {
    expect(splitAddress("/live?console=shown&service=sqs")).toEqual({ path: "/live", search: { console: "shown", service: "sqs" } });
    expect(splitAddress("/resources/sqs/queues/orders")).toEqual({ path: "/resources/sqs/queues/orders", search: {} });
  });
});
