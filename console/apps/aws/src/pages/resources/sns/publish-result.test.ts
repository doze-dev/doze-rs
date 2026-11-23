import { expect, test } from "bun:test";
import { type Published, publishedSentence } from "./publish-result";
import { plain } from "./sentences";
import type { Subscription } from "./sub-form-model";
import type { SubRow } from "./sub-model";

const row = (endpoint: string): SubRow => ({ name: "n", pending: false, topicArn: "", topic: "t", protocol: "sqs", endpoint, id: "x" });
const detail = (filter: string | null, scope: Subscription["scope"] = "MessageAttributes"): Subscription => ({ name: "n", arn: "a", topicArn: "", protocol: "sqs", endpoint: "", pending: false, raw: false, filter, scope, redrive: null, owner: null });
const sent = (event: string): Published => ({ messageId: "m", sequenceNumber: null, group: null, dedup: null, attributes: { event: { type: "String", value: event } }, custom: false, at: 0 });

test("the message is told to have gone to the subscriptions whose filter it satisfies", () => {
  const subs = [row("arn:aws:sqs:us-east-1:1:orders-events.fifo"), row("arn:aws:sqs:us-east-1:1:other")];
  const f = '{"event":["order_placed","order_cancelled"]}';
  expect(plain(publishedSentence(sent("order_placed"), subs, [detail(f), detail('{"event":["x"]}')]))).toBe("The message went to 1 of 2 confirmed subscriptions: orders-events.fifo. It matched the filter policy (event in [order_placed, order_cancelled]).");
  expect(plain(publishedSentence(sent("zzz"), subs.slice(0, 1), [detail(f)]))).toBe("The message went to 0 of 1 confirmed subscription.");
});

test("a body-scoped filter or a custom payload is the service's to decide, and the page says so", () => {
  const subs = [row("arn:aws:sqs:us-east-1:1:q")];
  expect(plain(publishedSentence(sent("x"), subs, [detail('{"a":["b"]}', "MessageBody")]))).toContain("a filter policy this page does not evaluate");
  expect(plain(publishedSentence(sent("x"), [], []))).toBe("The topic has no confirmed subscription, so the message reached nobody.");
});

test("a FIFO publish names its message group", () => {
  const subs = [row("arn:aws:sqs:us-east-1:1:orders-events.fifo")];
  expect(plain(publishedSentence({ ...sent("order_placed"), group: "order-10482" }, subs, [detail(null)]))).toBe("The message in group order-10482 went to 1 of 1 confirmed subscription: orders-events.fifo.");
});
