import { expect, test } from "bun:test";
import { matches, policyWords } from "./filter-match";

const s = (value: string) => ({ type: "String" as const, value });

test("a message whose attribute is one of the values matches, and one whose attribute is another does not", () => {
  const policy = '{"event":["order_placed","order_cancelled"]}';
  expect(matches(policy, { event: s("order_placed") })).toBe("match");
  expect(matches(policy, { event: s("order_shipped") })).toBe("no-match");
  expect(matches(policy, {})).toBe("no-match");
});

test("every key must be satisfied, and the rules prefix, anything-but, exists and numeric are read", () => {
  expect(matches('{"event":[{"prefix":"order_"}],"region":["eu"]}', { event: s("order_x"), region: s("eu") })).toBe("match");
  expect(matches('{"event":[{"prefix":"order_"}],"region":["eu"]}', { event: s("order_x"), region: s("us") })).toBe("no-match");
  expect(matches('{"event":[{"anything-but":["a"]}]}', { event: s("b") })).toBe("match");
  expect(matches('{"event":[{"exists":false}]}', {})).toBe("match");
  expect(matches('{"total":[{"numeric":[">",10]}]}', { total: { type: "Number", value: "41.8" } })).toBe("match");
  expect(matches('{"tags":["x"]}', { tags: { type: "String.Array", value: '["x","y"]' } })).toBe("match");
});

test("a policy this does not read is unknown, never a guess", () => {
  expect(matches('{"$or":[{"a":["1"]}]}', {})).toBe("unknown");
  expect(matches('{"a":[{"cidr":"10.0.0.0/8"}]}', { a: s("10.0.0.1") })).toBe("unknown");
  expect(matches("not json", {})).toBe("unknown");
});

test("a policy of plain values is said short", () => {
  expect(policyWords('{"event":["order_placed","order_cancelled"]}')).toBe("event in [order_placed, order_cancelled]");
  expect(policyWords('{"a":[{"prefix":"x"}]}')).toBe('{"a":[{"prefix":"x"}]}');
});
