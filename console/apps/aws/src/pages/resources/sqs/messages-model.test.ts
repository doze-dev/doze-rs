import { expect, test } from "bun:test";
import { countOf, deletedSentence, type Message, nearLimit, polledSentence } from "./messages-model";

const m = (id: string, count: number): Message => ({ MessageId: id, ReceiptHandle: "h", Body: "{}", Attributes: { ApproximateReceiveCount: String(count) } });

test("the count is 1 to 10", () => {
  expect(countOf("10")).toBe(10);
  expect(countOf("0")).toContain("at least 1");
  expect(countOf("x")).toContain("most");
});
test("a message near the dead-letter limit says so, with the console's part in it", () => {
  const w = nearLimit([m("0c91a7d3-6e54", 4), m("5b6c2f5e-9d1a", 1)], "pick-lists-dlq", 5, "pick-lists");
  expect(w).toHaveLength(1);
  expect(w[0]).toContain("0c91a7d3... has been received 4 times. pick-lists moves a message to pick-lists-dlq once its receive count exceeds 5.");
  expect(nearLimit([m("a", 9)], null, null, "q")).toEqual([]);
});
test("the poll and the delete are said in words", () => {
  expect(polledSentence(3, 3, 1)).toBe("The poll returned all 3 waiting messages. 1 more is in flight with another consumer.");
  expect(polledSentence(0, 0, 0)).toBe("The poll returned no messages.");
  expect(deletedSentence(1, "pick-lists", 2)).toBe("Deleted 1 message from pick-lists. It is gone for good; the other 2 messages from this poll are untouched.");
  expect(deletedSentence(2, "q", 0)).toBe("Deleted 2 messages from q. It is gone for good.");
});
