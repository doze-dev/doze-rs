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
