import { expect, test } from "bun:test";
import { statementCount, subscribeThenAllow, topicHint, withTopic } from "./subscribe-model";

const Q = "arn:aws:sqs:us-east-1:1:pick-lists";
const T = "arn:aws:sns:us-east-1:1:orders-created";

test("a queue with no policy gets one statement for the topic", () => {
  const r = withTopic(null, Q, T);
  expect(r.added).toBe(true);
  expect(r.count).toBe(1);
  const doc = JSON.parse(r.policy) as { Version: string; Statement: { Principal: { Service: string }; Condition: { ArnEquals: Record<string, string> } }[] };
  expect(doc.Version).toBe("2012-10-17");
  expect(doc.Statement[0]!.Principal.Service).toBe("sns.amazonaws.com");
  expect(doc.Statement[0]!.Condition.ArnEquals["aws:SourceArn"]).toBe(T);
});
test("a statement the queue has stays, and one for the same topic is not added twice", () => {
  const first = withTopic(JSON.stringify({ Version: "2012-10-17", Statement: [{ Sid: "x", Effect: "Allow", Principal: "*", Action: "sqs:*", Resource: Q }] }), Q, T);
  expect(first.count).toBe(2);
  expect(statementCount(first.policy)).toBe(2);
  const again = withTopic(first.policy, Q, T);
  expect(again.added).toBe(false);
  expect(again.count).toBe(2);
});

test("the other topics are named with what they already deliver", () => {
  expect(topicHint([])).toBe("The topics in this account and Region are listed. If yours isn't, enter its ARN.");
  expect(topicHint([{ name: "alerts", cells: ["Standard", "0"] }, { name: "orders-created", cells: ["Standard", "3"] }])).toContain("alerts, orders-created are the other topics here; orders-created already has 3 subscriptions.");
  // The topic chosen says what it already delivers too, before the others do.
  expect(topicHint([{ name: "alerts", cells: ["Standard", "1"] }], { name: "orders-created", cells: ["Standard", "3"] })).toContain("alerts is the other topic here; orders-created already has 3 subscriptions; alerts already has 1 subscription.");
  expect(topicHint([], { name: "orders-created", cells: ["Standard", "3"] })).toBe("The topics in this account and Region are listed. If yours isn't, enter its ARN. orders-created already has 3 subscriptions.");
});

test("a refused Subscribe leaves the policy alone: the statement is written only after the subscription is made", async () => {
  const calls: string[] = [];
  const allow = () => { calls.push("policy"); return Promise.resolve(); };
  await expect(subscribeThenAllow(() => { calls.push("subscribe"); return Promise.reject(new Error("Topic does not exist")); }, allow)).rejects.toThrow("Topic does not exist");
  expect(calls).toEqual(["subscribe"]);
  calls.length = 0;
  await subscribeThenAllow(() => { calls.push("subscribe"); return Promise.resolve(); }, allow);
  expect(calls).toEqual(["subscribe", "policy"]);
  calls.length = 0;
  await subscribeThenAllow(() => { calls.push("subscribe"); return Promise.resolve(); }, null);
  expect(calls).toEqual(["subscribe"]);
});
