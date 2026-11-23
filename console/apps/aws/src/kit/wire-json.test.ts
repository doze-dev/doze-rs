import { expect, test } from "bun:test";
import { unflatten } from "./wire-json";

test("a map's entries and a list's members come out nested", () => {
  expect(unflatten({ Name: "t", "Attributes.entry.1.key": "DisplayName", "Attributes.entry.1.value": "x", "Attributes.entry.2.key": "FifoTopic", "Attributes.entry.2.value": "true", "Tags.member.1.Key": "stack", "Tags.member.1.Value": "larder", "Tags.member.2.Key": "app", "Tags.member.2.Value": "fulfilment" })).toEqual({
    Name: "t",
    Attributes: { DisplayName: "x", FifoTopic: "true" },
    Tags: [{ Key: "stack", Value: "larder" }, { Key: "app", Value: "fulfilment" }],
  });
});

test("a body with no dotted key is left as it is, and a plain list is an array", () => {
  const plain = { TopicArn: "arn", Message: "m" };
  expect(unflatten(plain)).toBe(plain);
  expect(unflatten({ "TagKeys.member.1": "a", "TagKeys.member.2": "b" })).toEqual({ TagKeys: ["a", "b"] });
});

test("message attributes nest by their name", () => {
  expect(unflatten({ "MessageAttributes.entry.1.Name": "event", "MessageAttributes.entry.1.Value.DataType": "String", "MessageAttributes.entry.1.Value.StringValue": "order_placed" })).toEqual({ MessageAttributes: [{ Name: "event", Value: { DataType: "String", StringValue: "order_placed" } }] });
});
