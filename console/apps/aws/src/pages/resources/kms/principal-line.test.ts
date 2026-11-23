import { describe, expect, test } from "bun:test";
import { principalLine, resourceWords } from "./principal-line";

const ARN = "arn:aws:iam::123456789012:policy/report-job-readonly";
const readonly = { Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: "s3:GetObject", Resource: "arn:aws:s3:::reports/*" }] };

describe("principalLine", () => {
  test("one policy that grants one S3 read is told as the board tells it", () => {
    const line = principalLine({ name: "report-job", attached: [{ PolicyName: "report-job-readonly", PolicyArn: ARN }], inline: [], docs: new Map([[ARN, readonly]]) });
    expect(line).toBe("report-job reads reports/* in S3 through its report-job-readonly policy.");
  });
  test("another single action names the action and the resource", () => {
    const doc = { Statement: [{ Effect: "Allow", Action: "sqs:SendMessage", Resource: "arn:aws:sqs:us-east-1:123456789012:pick-lists" }] };
    const line = principalLine({ name: "ci-bot", attached: [{ PolicyName: "send", PolicyArn: ARN }], inline: [], docs: new Map([[ARN, doc]]) });
    expect(line).toBe("ci-bot is allowed sqs:SendMessage on pick-lists through its send policy.");
  });
  test("no policy says so, and what the key policy does", () => {
    expect(principalLine({ name: "dev", attached: [], inline: [], docs: new Map() })).toContain("the key policy is all that lets it use this key");
  });
  test("several policies, an inline one, or an AWS managed one are listed, not summarised", () => {
    const aws = "arn:aws:iam::aws:policy/ReadOnlyAccess";
    expect(principalLine({ name: "dev", attached: [{ PolicyName: "ReadOnlyAccess", PolicyArn: aws }], inline: [], docs: new Map() })).toBe("dev holds ReadOnlyAccess, which says what it can do beside this key.");
    expect(principalLine({ name: "dev", attached: [{ PolicyName: "a", PolicyArn: ARN }], inline: ["b"], docs: new Map([[ARN, readonly]]) })).toBe("dev holds a and b, which say what it can do beside this key.");
  });
  test("a policy with several actions is listed too", () => {
    const doc = { Statement: [{ Effect: "Allow", Action: ["s3:GetObject", "s3:PutObject"], Resource: "*" }] };
    expect(principalLine({ name: "x", attached: [{ PolicyName: "p", PolicyArn: ARN }], inline: [], docs: new Map([[ARN, doc]]) })).toBe("x holds p, which says what it can do beside this key.");
  });
  test("resourceWords", () => {
    expect(resourceWords("arn:aws:s3:::reports/*")).toBe("reports/*");
    expect(resourceWords("*")).toBe("*");
  });
});
