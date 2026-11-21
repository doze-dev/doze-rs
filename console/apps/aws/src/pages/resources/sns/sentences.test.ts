import { expect, test } from "bun:test";
import { plain, rawWords, subscriptionsSentence, topicsLeftSentence, topicsSentence } from "./sentences";
import type { SubRow } from "./sub-model";

const row = (over: Partial<SubRow>): SubRow => ({ name: "n", pending: false, topicArn: "", topic: "orders-created", protocol: "sqs", endpoint: "arn:aws:sqs:us-east-1:1:q", id: "x", ...over });
const pending = [row({ pending: true, topic: "alerts", protocol: "http", endpoint: "http://127.0.0.1:9000/in", id: null }), row({ pending: true, topic: "alerts", protocol: "email", endpoint: "ops@larder.example", id: null })];
const subs = [...Array.from({ length: 5 }, () => row({})), ...pending];

test("the topics sentence counts, says what waits and where, and the last publish", () => {
  expect(plain(topicsSentence(3, subs, "us-east-1", { topic: "orders-created", at: "08:13:58" }))).toBe("3 topics and 7 subscriptions in us-east-1. 2 are pending, both on alerts: the webhook http://127.0.0.1:9000/in and the stored email ops@larder.example, which stays pending. Last publish: orders-created at 08:13:58.");
  expect(plain(topicsSentence(1, [row({})], "us-east-1", null))).toBe("1 topic and 1 subscription in us-east-1.");
  expect(plain(topicsSentence(0, [], "us-east-1", null))).toContain("ListTopics returned an empty list");
});

test("the subscriptions sentence says each pending one's reason", () => {
  expect(plain(subscriptionsSentence(subs))).toBe("7 subscriptions, 5 confirmed and 2 pending, both on alerts: the webhook http://127.0.0.1:9000/in is waiting for its confirmation, and ops@larder.example is stored and stays pending because doze-aws sends no email.");
  expect(plain(subscriptionsSentence([row({}), row({})]))).toBe("2 subscriptions, all confirmed.");
});

test("a deleted topic leaves a sentence of what publishers now get", () => {
  expect(plain(topicsLeftSentence(2, "us-east-1", "orders-created"))).toBe("2 topics left in us-east-1. A publish to orders-created now gets NotFound: the topic does not exist.");
});

test("raw delivery is said when it is the same on every subscription, and not when it differs", () => {
  expect(rawWords([false, false, false], ["sqs", "sqs", "sqs"])).toBe("Raw message delivery is off on all three, so each queue receives the full JSON envelope.");
  expect(rawWords([false, true], ["sqs", "sqs"])).toBeNull();
  expect(rawWords([], [])).toBeNull();
});
