import { expect, test } from "bun:test";
import { countOf, deletedBanner, deletedNote, leftSentence, type Message, nearLimit, polledSentence, sizeOf } from "./messages-model";

const m = (id: string, count: number): Message => ({ MessageId: id, ReceiptHandle: "h", Body: "{}", Attributes: { ApproximateReceiveCount: String(count) } });

test("the count is 1 to 10", () => {
  expect(countOf("10")).toBe(10);
  expect(countOf("0")).toContain("at least 1");
  expect(countOf("x")).toContain("most");
});
test("a message near the dead-letter limit says so, once, with the console's part in it", () => {
  const w = nearLimit([m("0c91a7d3-6e54", 4), m("5b6c2f5e-9d1a", 1)], "pick-lists-dlq", 5, "pick-lists");
  expect(w?.lead).toBe("0c91a7d3... has been received 4 times.");
  expect(w?.text).toBe("pick-lists moves a message to pick-lists-dlq once its receive count exceeds 5. The console's own polling counts as a receive.");
  expect(nearLimit([m("aaaaaaaa-1", 4), m("bbbbbbbb-2", 5)], "d", 5, "q")?.lead).toBe("2 messages are close to the limit: aaaaaaaa... (4 times), bbbbbbbb... (5 times).");
  expect(nearLimit([m("a", 9)], null, null, "q")).toBeNull();
  expect(nearLimit([m("a", 1)], "d", 5, "q")).toBeNull();
});
test("the poll and the delete are said in words", () => {
  expect(polledSentence(3, 3, 1)).toBe("The poll returned all 3 waiting messages. 1 more is in flight with another consumer.");
  expect(polledSentence(0, 0, 0)).toBe("The poll returned no messages.");
  expect(deletedBanner(1, "pick-lists", 2)).toEqual({ title: "Deleted 1 message from pick-lists.", returned: "It is gone for good; the other two messages from this poll are untouched." });
  expect(deletedBanner(2, "q", 0)).toEqual({ title: "Deleted 2 messages from q.", returned: "They are gone for good." });
  expect(deletedBanner(1, "q", 1).returned).toBe("It is gone for good; the other message from this poll is untouched.");
});
test("what is left of a poll never claims what it no longer holds", () => {
  expect(leftSentence(2)).toBe("2 messages are left from the last poll.");
  expect(leftSentence(1)).toBe("1 message is left from the last poll.");
  expect(leftSentence(0)).toBe("No message is left from the last poll.");
  expect(deletedNote(4, "pick-lists-dlq")).toBe("The deleted message was received 4 times and would have gone to pick-lists-dlq on its next failed receives. Deleting needs the receipt handle from its most recent receive, which this poll supplied.");
  expect(deletedNote(null, null)).toBe("Deleting needs the receipt handle from its most recent receive, which this poll supplied.");
});
test("a size is in bytes under a kibibyte", () => {
  expect(sizeOf({ ...m("a", 1), Body: "x".repeat(53) })).toBe("53 bytes");
  expect(sizeOf({ ...m("a", 1), Body: "x" })).toBe("1 byte");
  expect(sizeOf({ ...m("a", 1), Body: "x".repeat(2048) })).toBe("2.0 KiB");
});
