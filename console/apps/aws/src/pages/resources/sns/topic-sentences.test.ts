import { expect, test } from "bun:test";
import { plain } from "./sentences";
import type { SubRow } from "./sub-model";
import { accessSentence, archiveSentence, encryptionSentence, loggingSentence, retrySentence, tagsSentence, topicSentence } from "./topic-sentences";
import type { Topic } from "./topic-model";

const sub = (over: Partial<SubRow>): SubRow => ({ name: "n", pending: false, topicArn: "", topic: "t", protocol: "sqs", endpoint: "arn:aws:sqs:us-east-1:1:q", id: "x", ...over });
const t = { name: "orders-created", fifo: false };

test("a topic says where it sends every publish, and what raw delivery does", () => {
  const subs = ["invoice-queue", "inventory-queue", "analytics-queue"].map((q) => sub({ endpoint: `arn:aws:sqs:us-east-1:1:${q}` }));
  expect(plain(topicSentence(t, subs, "Raw message delivery is off on all three, so each queue receives the full JSON envelope.", null))).toBe("orders-created sends every publish to its 3 confirmed subscriptions: analytics-queue, inventory-queue and invoice-queue. Raw message delivery is off on all three, so each queue receives the full JSON envelope.");
});

test("a topic with some subscriptions waiting says which, and who a publish reaches", () => {
  const subs = [sub({ protocol: "lambda", endpoint: "arn:aws:lambda:us-east-1:1:function:send-email" }), sub({ pending: true, protocol: "http", endpoint: "http://127.0.0.1:9000/in", id: null }), sub({ pending: true, protocol: "email", endpoint: "ops@larder.example", id: null })];
  expect(plain(topicSentence({ name: "alerts", fifo: false }, subs, null, null))).toBe("alerts has 3 subscriptions: the Lambda function send-email is confirmed, the webhook http://127.0.0.1:9000/in waits for its confirmation, and ops@larder.example is stored and stays pending because doze-aws sends no email. A publish reaches only the confirmed one.");
});

test("a topic with none is told as a dead end, and a new FIFO topic says which queue to subscribe", () => {
  expect(plain(topicSentence(t, [], null, null))).toBe("orders-created has no subscriptions, so anything published to it goes nowhere.");
  expect(plain(topicSentence({ name: "orders-events.fifo", fifo: true }, [], null, { queue: "orders-events.fifo" }))).toBe("orders-events.fifo has no subscriptions yet, so a publish would reach nobody. Subscribe the FIFO queue orders-events.fifo from SQS to start delivering.");
});

test("each tab has its own sentence, and what is only stored is said so", () => {
  const policy = JSON.stringify({ Statement: [{ Effect: "Allow", Principal: { AWS: "*" }, Condition: { StringEquals: { "AWS:SourceOwner": "111122223333" } } }] });
  expect(plain(accessSentence("orders-created", policy, "111122223333"))).toBe("orders-created is open to its owner only: one statement allows any principal in account 111122223333 to publish, subscribe and manage the topic.");
  expect(plain(retrySentence("orders-created", null, 0))).toBe("orders-created sets no delivery policy of its own, and it has no HTTP/S subscriptions. Amazon SQS and AWS Lambda deliveries are one attempt.");
  expect(plain(retrySentence("alerts", null, 1))).toContain("1 HTTP/S subscription is retried under the defaults");
  const topic = { name: "orders-created", feedback: { HTTP: { success: null, rate: null, failure: null }, Lambda: { success: null, rate: null, failure: null }, SQS: { success: "arn:aws:iam::1:role/r", rate: 100, failure: "arn:aws:iam::1:role/r" } } } as Pick<Topic, "name" | "feedback">;
  expect(plain(loggingSentence(topic))).toContain("delivery status logging for Amazon SQS only, with 100% of successful deliveries sampled. Nothing is written to CloudWatch Logs locally");
  expect(plain(encryptionSentence("orders-created", "alias/larder-app"))).toContain("Nothing is encrypted locally");
  expect(plain(tagsSentence("orders-created", ["stack", "app"])!)).toContain("carries 2 tags: stack and app.");
  expect(tagsSentence("alerts", [])).toBeNull();
  expect(plain(archiveSentence("orders-events.fifo", '{"MessageRetentionPeriod":"7"}'))).toContain("an archive policy of 7 days");
});
