import { expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { changePhrases, changes, createInput, defaultPolicy, deliveryPolicy, DEFAULT_RETRY, emptyTopicForm, formOfTopic, problems, retryOf } from "./topic-form-model";
import { topicOf } from "./topic-model";

const R = "us-east-1";
const A = "111122223333";
const detail = (fields: Record<string, string>): ResourceDetail => ({ name: "orders-created", id: `arn:aws:sns:${R}:${A}:orders-created`, fields: Object.entries(fields).map(([label, value]) => ({ label, value })) }) as unknown as ResourceDetail;

test("a create sends only what differs from the service's defaults", () => {
  const f = { ...emptyTopicForm(), name: "deliveries-updated", display: "larder deliveries", sse: true, kms: "alias/larder-app", tags: [newRow("stack", "larder")] };
  expect(createInput(f, R, A)).toEqual({ Name: "deliveries-updated", Attributes: { DisplayName: "larder deliveries", KmsMasterKeyId: "alias/larder-app" }, Tags: [{ Key: "stack", Value: "larder" }] });
  expect(createInput({ ...emptyTopicForm(), name: "plain" }, R, A)).toEqual({ Name: "plain" });
});

test("a FIFO create sends its type, deduplication and scope, and the default policy is not sent", () => {
  const f = { ...emptyTopicForm(), fifo: true, name: "orders-events.fifo", dedup: true, scope: "MessageGroup" as const, policy: defaultPolicy("orders-events.fifo", R, A) };
  expect(createInput(f, R, A)).toEqual({ Name: "orders-events.fifo", Attributes: { FifoTopic: "true", ContentBasedDeduplication: "true", FifoThroughputScope: "MessageGroup" } });
});

test("a changed retry policy is one DeliveryPolicy, and logging sends its roles and rate", () => {
  const f = { ...emptyTopicForm(), name: "t", retry: { ...DEFAULT_RETRY, numRetries: "7", maxReceives: "5" }, logging: { ...emptyTopicForm().logging, SQS: { on: true, rate: "50", success: "arn:aws:iam::1:role/r", failure: "arn:aws:iam::1:role/r" } } };
  const attrs = (createInput(f, R, A).Attributes ?? {}) as Record<string, string>;
  expect(JSON.parse(attrs.DeliveryPolicy!)).toEqual({ http: { defaultHealthyRetryPolicy: { minDelayTarget: 20, maxDelayTarget: 20, numRetries: 7, numMaxDelayRetries: 0, numNoDelayRetries: 0, numMinDelayRetries: 0, backoffFunction: "linear" }, defaultThrottlePolicy: { maxReceivesPerSecond: 5 } } });
  expect(attrs).toMatchObject({ SQSSuccessFeedbackRoleArn: "arn:aws:iam::1:role/r", SQSSuccessFeedbackSampleRate: "50", SQSFailureFeedbackRoleArn: "arn:aws:iam::1:role/r" });
  expect(retryOf(deliveryPolicy(f.retry))).toEqual(f.retry);
});

test("an edit is one call for each setting that changed, and nothing for what did not", () => {
  const t = topicOf(detail({ ARN: `arn:aws:sns:${R}:${A}:orders-created`, Type: "Standard", "Display name": "larder orders", "KMS key": "alias/larder-app", "SQSSuccessFeedbackRoleArn": "arn:aws:iam::1:role/r", "SQSSuccessFeedbackSampleRate": "100" }));
  const f = formOfTopic(t, [], R, A);
  expect(changes(f, t, R, A)).toEqual([]);
  expect(changes({ ...f, maxKib: "512", display: "orders" }, t, R, A)).toEqual([{ name: "DisplayName", value: "orders" }, { name: "MaximumMessageSize", value: "524288" }]);
  expect(changes({ ...f, sse: false }, t, R, A)).toEqual([{ name: "KmsMasterKeyId", value: "" }]);
  expect(changes({ ...f, logging: { ...f.logging, SQS: { ...f.logging.SQS, on: false } } }, t, R, A).map((c) => c.name)).toEqual(["SQSSuccessFeedbackRoleArn", "SQSSuccessFeedbackSampleRate"]);
});

test("the form sees a missing name, a bad size, encryption without a key, and a retry out of range", () => {
  expect(problems(emptyTopicForm(), true).name).toBe("Enter a name.");
  expect(problems({ ...emptyTopicForm(), name: "orders-events", fifo: true }, true).name).toBeUndefined();
  expect(problems({ ...emptyTopicForm(), name: "a", maxKib: "2000" }, true).maxKib).toContain("1,024");
  expect(problems({ ...emptyTopicForm(), name: "a", sse: true }, true).kms).toContain("KMS");
  expect(problems({ ...emptyTopicForm(), name: "a", retry: { ...DEFAULT_RETRY, numRetries: "101" } }, true).retry).toContain("0 to 100");
});

test("an edit says what each change does", () => {
  expect(changePhrases([{ name: "MaximumMessageSize", value: "524288" }, { name: "SQSSuccessFeedbackRoleArn", value: "" }, { name: "SQSFailureFeedbackRoleArn", value: "" }, { name: "KmsMasterKeyId", value: "" }])).toEqual(["the maximum message size goes to 512 KiB", "delivery status logging changes", "encryption is turned off"]);
});
