import { expect, test } from "bun:test";
import { answerWords } from "./answer";

const ok = (action: string, answer_bytes: number | null) => ({ action, answer_bytes, outcome: "ok" as const, function_error: null });

test("an empty receive says empty, a receive with an answer says it has messages", () => {
  expect(answerWords(ok("ReceiveMessage", 2))).toBe("empty");
  expect(answerWords(ok("ReceiveMessage", 480))).toBe("has messages");
});
test("a read of nothing says so, any other answer its size", () => {
  expect(answerWords(ok("ListQueues", 2))).toBe("nothing in the answer");
  expect(answerWords(ok("SendMessage", 106))).toBe("106 B");
});
test("an answer that declared no length, a failure and a failed function say nothing of the kind", () => {
  expect(answerWords(ok("ReceiveMessage", null))).toBeNull();
  expect(answerWords({ ...ok("Invoke", 73), function_error: "Unhandled" })).toBeNull();
  expect(answerWords({ ...ok("GetItem", 73), outcome: "error" as never })).toBeNull();
});

test("a summary the recorder read says what the call did, before any size", () => {
  expect(answerWords({ ...ok("ReceiveMessage", 480), summary: "3 messages" })).toBe("3 messages");
  expect(answerWords({ ...ok("SendMessage", 106), summary: "sent 53 B" })).toBe("sent 53 B");
  // A summary needs no declared length (a call between services has none).
  expect(answerWords({ ...ok("GetObject", null), summary: "212 KiB" })).toBe("212 KiB");
});
test("a failure and a failed function say nothing of the summary", () => {
  expect(answerWords({ ...ok("ReceiveMessage", 2), outcome: "error" as never, summary: "no messages" })).toBeNull();
  expect(answerWords({ ...ok("Invoke", 73), function_error: "Handled", summary: "1 event" })).toBeNull();
});
