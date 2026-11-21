import { expect, test } from "bun:test";
import { sourceWords, whoLetsIn } from "./policy-reading";

const policy = JSON.stringify({ Statement: [{ Effect: "Allow", Principal: { Service: "events.amazonaws.com" }, Action: "sqs:SendMessage", Condition: { ArnEquals: { "aws:SourceArn": "arn:aws:events:us-east-1:1:rule/larder/payment-settled" } } }] });

test("the owner is always let in, then each principal with what and from where", () => {
  expect(whoLetsIn(null, "123")).toEqual([{ who: "Queue owner", what: "Account 123: send and receive, by default" }]);
  expect(whoLetsIn(policy, "123")[1]).toEqual({ who: "EventBridge", what: "SendMessage, only from the rule payment-settled" });
});
test("everyone, and a refusal, and a policy that is not JSON", () => {
  const open = JSON.stringify({ Statement: { Effect: "Allow", Principal: "*", Action: ["sqs:SendMessage", "sqs:ReceiveMessage", "sqs:DeleteMessage"] } });
  expect(whoLetsIn(open, "1")[1]).toEqual({ who: "Everyone", what: "Send and receive" });
  const deny = JSON.stringify({ Statement: [{ Effect: "Deny", Principal: { AWS: "arn:aws:iam::2:user/bob" }, Action: "sqs:*" }] });
  expect(whoLetsIn(deny, "1")[1]).toEqual({ who: "user/bob", what: "Refused: every action" });
  expect(whoLetsIn("{nope", "1")).toHaveLength(1);
});
test("sources are named by what they are", () => {
  expect(sourceWords("arn:aws:sns:us-east-1:1:orders-created")).toBe("the topic orders-created");
  expect(sourceWords("not an arn")).toBe("not an arn");
});
