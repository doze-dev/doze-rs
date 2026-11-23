import { expect, test } from "bun:test";
import { andList, chipWords, firstAccepted, madeSentence, purgedSentence, sourceSentence } from "./sentences";

test("a dead-letter queue names the limit of its one source", () => {
  const s = "pick-lists-dlq holds 1 message. It is the dead-letter queue of pick-lists.";
  expect(sourceSentence(s, ["pick-lists"], 5)).toBe("pick-lists-dlq holds 1 message. It is the dead-letter queue of pick-lists, which sends a message here once its receive count exceeds 5.");
  expect(sourceSentence(s, ["a", "b"], 5)).toBe(s);
  expect(sourceSentence(null, ["a"], 5)).toBeNull();
});
test("a new FIFO queue says what its first message needs", () => {
  expect(madeSentence("x.fifo", true)).toContain("message group ID");
  expect(purgedSentence("q", 0, 0)).toBe("q was purged: 0 messages waiting and 0 in flight. The queue and its settings are unchanged.");
});

test("a wired chip is set as the board sets it", () => {
  const to = (name: string) => ({ service: "x", kind: "y", name });
  expect(chipWords({ label: "Fed by", to: to("larder/payment-settled") })).toBe("payment-settled rule, bus larder");
  expect(chipWords({ label: "Fed by", to: to("nightly") })).toBe("nightly rule");
  expect(chipWords({ label: "KMS key", to: to("alias/larder-app") })).toBe("alias/larder-app key");
  expect(chipWords({ label: "Dead-letter queue", to: to("pick-lists-dlq") })).toBe("pick-lists-dlq");
});

test("a list is joined with and", () => {
  expect(andList([])).toBe("");
  expect(andList(["a"])).toBe("a");
  expect(andList(["a", "b"])).toBe("a and b");
  expect(andList(["a", "b", "c"])).toBe("a, b and c");
});

test("a trigger form opens on the first function the queue accepts", () => {
  expect(firstAccepted(["export-report", "send-email"], [120, 30], 30)).toBe("send-email");
  expect(firstAccepted(["export-report", "send-email"], [null, 30], 30)).toBe("send-email");
  expect(firstAccepted(["export-report", "send-email"], [120, null], 30)).toBeUndefined();
  expect(firstAccepted(["export-report", "send-email"], [120, 300], 30)).toBe("export-report");
  expect(firstAccepted([], [], 30)).toBeNull();
});
