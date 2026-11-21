import { expect, test } from "bun:test";
import { emptyPublish, givenText, newAttr, problems, publishInput } from "./publish-model";

const topic = { fifo: false, maxSize: 262_144 };

test("an identical payload sends the message, the subject and the attributes", () => {
  const f = { ...emptyPublish(), body: "hello", subject: "Order 1", attrs: [newAttr("event", "order_placed")] };
  expect(publishInput(f)).toEqual({ Message: "hello", Subject: "Order 1", MessageAttributes: { event: { DataType: "String", StringValue: "order_placed" } } });
});

test("a custom payload is MessageStructure json and drops the attributes", () => {
  const f = { ...emptyPublish(), structure: "custom" as const, custom: '{"default":"x","sqs":"y"}', attrs: [newAttr("event", "e")] };
  expect(publishInput(f)).toEqual({ Message: '{"default":"x","sqs":"y"}', MessageStructure: "json" });
  expect(givenText(f, "sqs")).toBe("the sqs text");
  expect(givenText(f, "lambda")).toBe("the default text");
  expect(problems({ ...f, custom: '{"sqs":"y"}' }, topic).custom).toContain("default");
});

test("a FIFO topic needs a group, and a missing deduplication ID is the service's to refuse", () => {
  const f = { ...emptyPublish(), body: "x" };
  expect(problems(f, { fifo: true, maxSize: 262_144 }).group).toBe("Required on a FIFO topic.");
  expect(problems({ ...f, group: "order-1" }, { fifo: true, maxSize: 262_144 })).toEqual({});
  expect(publishInput({ ...f, group: "order-1", dedup: "d1" })).toMatchObject({ MessageGroupId: "order-1", MessageDeduplicationId: "d1" });
});

test("the size is checked against the topic's limit, in bytes", () => {
  expect(problems({ ...emptyPublish(), body: "x".repeat(2000) }, { fifo: false, maxSize: 1024 }).size).toContain("2,000 bytes");
  expect(problems({ ...emptyPublish() }, topic).body).toBe("Enter the message.");
});
