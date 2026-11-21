import { expect, test } from "bun:test";
import { statementCount, withTopic } from "./subscribe-model";

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
