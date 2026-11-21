import { expect, test } from "bun:test";
import { listOf, queueEndpoints, subscribedWords } from "./sns-words";

test("a topic's subscribers are named when they are known, counted when they are not", () => {
  const q = queueEndpoints(["arn:aws:sqs:us-east-1:1:invoice-queue", "arn:aws:sqs:us-east-1:1:inventory-queue", "arn:aws:sqs:us-east-1:1:analytics-queue"]);
  expect(subscribedWords("orders-created", 3, q)).toBe("The topic orders-created delivers to analytics-queue, inventory-queue and invoice-queue today.");
  expect(subscribedWords("orders-created", 3, [])).toBe("The topic orders-created delivers to 3 subscriptions today.");
  expect(subscribedWords("t", 1, ["a"])).toBe("The topic t delivers to a today.");
  // An email among the endpoints: the names would not add up, so it counts.
  expect(subscribedWords("t", 2, queueEndpoints(["arn:aws:sqs:us-east-1:1:a", "me@example.com"]))).toBe("The topic t delivers to 2 subscriptions today.");
});

test("a list reads as a sentence", () => {
  expect(listOf(["a"])).toBe("a");
  expect(listOf(["a", "b"])).toBe("a and b");
});
