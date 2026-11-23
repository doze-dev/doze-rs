import { describe, expect, test } from "bun:test";
import { checkPolicy, statementWords } from "./policy";

const good = JSON.stringify({ Version: "2012-10-17", Statement: [{ Sid: "a", Effect: "Allow", Action: ["sqs:SendMessage", "sqs:ReceiveMessage"], Resource: "arn:aws:sqs:us-east-1:1:q" }] });

describe("checkPolicy", () => {
  test("a sound identity policy has no problem and one statement", () => {
    const c = checkPolicy(good);
    expect(c.ok).toBe(true);
    expect(c.statements).toHaveLength(1);
    expect(statementWords(c.statements[0]!)).toBe("Allow sqs:SendMessage, sqs:ReceiveMessage on arn:aws:sqs:us-east-1:1:q");
  });
  test("not JSON, empty, not an object", () => {
    expect(checkPolicy("{").problems[0]).toStartWith("It is not JSON");
    expect(checkPolicy("  ").problems).toEqual(["The policy is empty."]);
    expect(checkPolicy("[]").ok).toBe(false);
  });
  test("what is missing is named per statement", () => {
    const c = checkPolicy(JSON.stringify({ Statement: [{ Effect: "Permit" }] }));
    expect(c.problems).toEqual(['It has no Version; use "2012-10-17".', "Statement 1 needs an Effect of Allow or Deny.", "Statement 1 needs an Action.", "Statement 1 needs a Resource."]);
  });
  test("a single statement object is accepted, and a duplicate Sid is named", () => {
    const one = checkPolicy(JSON.stringify({ Version: "2012-10-17", Statement: { Effect: "Allow", Action: "s3:*", Resource: "*" } }));
    expect(one.ok).toBe(true);
    const dup = checkPolicy(JSON.stringify({ Version: "2012-10-17", Statement: [{ Sid: "x", Effect: "Allow", Action: "s3:*", Resource: "*" }, { Sid: "x", Effect: "Allow", Action: "s3:*", Resource: "*" }] }));
    expect(dup.problems).toEqual(['Sid "x" is used twice.']);
  });
  test("a resource policy needs a Principal; an identity policy must not have one", () => {
    const noPrincipal = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: "sqs:SendMessage", Resource: "*" }] });
    expect(checkPolicy(noPrincipal, "resource").problems).toEqual(["Statement 1 needs a Principal: a resource policy says who it is for."]);
    const withP = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { AWS: "arn:aws:iam::1:root" }, Action: "sqs:SendMessage", Resource: "*" }] });
    expect(checkPolicy(withP, "resource").ok).toBe(true);
    expect(checkPolicy(withP, "identity").ok).toBe(false);
    expect(statementWords(checkPolicy(withP, "resource").statements[0]!)).toBe("Allow sqs:SendMessage on * for AWS: arn:aws:iam::1:root");
  });
  test("size is counted without whitespace", () => {
    expect(checkPolicy(good, "identity", 50).problems[0]).toStartWith("It is ");
    expect(checkPolicy(good, "identity", 6144).ok).toBe(true);
  });
});
