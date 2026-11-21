import { expect, test } from "bun:test";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { archiveOf, deliveryRows, hasOwnDelivery, logged, roleName, topicOf } from "./topic-model";

const detail = (fields: Record<string, string | null>): ResourceDetail =>
  ({ name: "orders-created", id: "arn:aws:sns:us-east-1:1:orders-created", fields: Object.entries(fields).map(([label, value]) => ({ label, value })) }) as unknown as ResourceDetail;

test("a topic is read from its fields, with the service's defaults for what it sets none of", () => {
  const t = topicOf(detail({ ARN: "arn:aws:sns:us-east-1:1:orders-created", Type: "Standard", "Display name": "larder orders", "Confirmed subscriptions": "3", "Pending subscriptions": "0" }));
  expect(t).toMatchObject({ name: "orders-created", fifo: false, display: "larder orders", confirmed: 3, pending: 0, maxSize: 262_144, maxSizeSet: false, scope: "Topic", tracing: false, kms: null });
  const fifo = topicOf(detail({ Type: "FIFO", "FIFO throughput scope": "MessageGroup", "Maximum message size": "524288", "Active tracing": "Active", "Content-based deduplication": "Enabled" }));
  expect(fifo).toMatchObject({ fifo: true, scope: "MessageGroup", maxSize: 524_288, maxSizeSet: true, tracing: true, dedup: true });
});

test("delivery status logging is read per protocol, and a role is the switch", () => {
  const t = topicOf(detail({ SQSSuccessFeedbackRoleArn: "arn:aws:iam::1:role/sns-delivery-logs", SQSSuccessFeedbackSampleRate: "100", SQSFailureFeedbackRoleArn: "arn:aws:iam::1:role/sns-delivery-logs" }));
  expect(logged(t.feedback.SQS)).toBe(true);
  expect(logged(t.feedback.HTTP)).toBe(false);
  expect(t.feedback.SQS.rate).toBe(100);
  expect(roleName("arn:aws:iam::1:role/sns-delivery-logs")).toBe("role/sns-delivery-logs");
});

test("the delivery policy tab sets out each attribute with its value", () => {
  const effective = JSON.stringify({ http: { defaultHealthyRetryPolicy: { minDelayTarget: 20, maxDelayTarget: 20, numRetries: 3, numMaxDelayRetries: 0, numNoDelayRetries: 0, numMinDelayRetries: 0, backoffFunction: "linear" }, defaultRequestPolicy: { headerContentType: "text/plain; charset=UTF-8" } } });
  const rows = deliveryRows(effective);
  expect(rows.map((r) => [r.attribute, r.value])).toEqual([["numRetries", "3"], ["numNoDelayRetries", "0"], ["minDelayTarget", "20 seconds"], ["numMinDelayRetries", "0"], ["maxDelayTarget", "20 seconds"], ["numMaxDelayRetries", "0"], ["backoffFunction", "linear"], ["maxReceivesPerSecond", "No throttling"], ["headerContentType", "text/plain; charset=UTF-8"]]);
  expect(hasOwnDelivery(null)).toBe(false);
  expect(hasOwnDelivery("{}")).toBe(false);
  expect(hasOwnDelivery('{"http":{}}')).toBe(true);
});

test("an archive policy is active with a retention, and an empty one is not", () => {
  expect(archiveOf('{"MessageRetentionPeriod":"7"}')).toEqual({ active: true, days: 7 });
  expect(archiveOf("{}")).toEqual({ active: false, days: null });
  expect(archiveOf(null).active).toBe(false);
});
