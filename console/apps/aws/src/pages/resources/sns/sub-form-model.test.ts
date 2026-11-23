import { expect, test } from "bun:test";
import { changes, changeWords, confirmationOf, emptySubForm, formOfSubscription, problems, subscribeInput, type Subscription } from "./sub-form-model";

const TOPIC = "arn:aws:sns:us-east-1:111122223333:orders-events.fifo";
const sub = (over: Partial<Subscription> = {}): Subscription => ({ name: "n", arn: `${TOPIC}:abc`, topicArn: TOPIC, protocol: "sqs", endpoint: "arn:aws:sqs:us-east-1:111122223333:orders-events.fifo", pending: false, raw: false, filter: '{"event":["order_placed"]}', scope: "MessageAttributes", redrive: null, owner: "1", ...over });

test("Subscribe names the subscription and sends the attributes that differ from the defaults", () => {
  const f = { ...emptySubForm(TOPIC), endpoint: "arn:aws:sqs:us-east-1:111122223333:orders-events.fifo", filter: '{ "event": ["a"] }', raw: true };
  expect(subscribeInput(f)).toEqual({ TopicArn: TOPIC, Protocol: "sqs", Endpoint: f.endpoint, ReturnSubscriptionArn: true, Attributes: { RawMessageDelivery: "true", FilterPolicy: '{"event":["a"]}', FilterPolicyScope: "MessageAttributes" } });
  // A filter policy says its scope and raw delivery even when they are the defaults (the board's call draws both).
  expect(subscribeInput({ ...f, raw: false, scope: "MessageBody" })).toMatchObject({ Attributes: { RawMessageDelivery: "false", FilterPolicyScope: "MessageBody" } });
  expect(subscribeInput({ ...emptySubForm(TOPIC), endpoint: f.endpoint })).not.toHaveProperty("Attributes");
  expect(subscribeInput({ ...emptySubForm(TOPIC), protocol: "http", endpoint: "http://127.0.0.1:9000/in", raw: true })).toMatchObject({ Attributes: { RawMessageDelivery: "true" } });
  expect(subscribeInput({ ...emptySubForm(TOPIC), protocol: "lambda", endpoint: "arn:aws:lambda:us-east-1:1:function:f", raw: true })).not.toHaveProperty("Attributes");
});

test("the endpoint is checked for its protocol", () => {
  expect(problems({ ...emptySubForm(TOPIC), protocol: "http", endpoint: "https://x" }, true).endpoint).toContain("http://");
  expect(problems({ ...emptySubForm(""), endpoint: "arn:aws:sqs:us-east-1:111122223333:q" }, true).topic).toContain("topic");
  expect(problems({ ...emptySubForm(TOPIC), endpoint: "arn:aws:sqs:us-east-1:111122223333:q" }, true)).toEqual({});
});

test("an edit is one call for each setting changed, said in words", () => {
  const s = sub();
  const f = formOfSubscription(s);
  expect(changes(f, s)).toEqual([]);
  const c = changes({ ...f, raw: true, redriveOn: true, dlq: "arn:aws:sqs:us-east-1:111122223333:orders-events-dlq.fifo" }, s);
  expect(c.map((x) => x.name)).toEqual(["RawMessageDelivery", "RedrivePolicy"]);
  expect(changeWords(c)).toBe("You turned raw message delivery on and set a dead-letter queue.");
  expect(changes({ ...f, filter: "" }, s)).toEqual([{ name: "FilterPolicy", value: "" }]);
});

test("the token is read out of a SubscribeURL", () => {
  expect(confirmationOf("http://127.0.0.1:4566/_aws/sns/confirm?Action=ConfirmSubscription&TopicArn=arn%3Aaws%3Asns%3Aus-east-1%3A1%3Aalerts&Token=abc123")).toEqual({ topicArn: "arn:aws:sns:us-east-1:1:alerts", token: "abc123" });
  expect(confirmationOf("nothing")).toBeNull();
});
