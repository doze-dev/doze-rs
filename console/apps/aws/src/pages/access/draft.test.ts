import { describe, expect, test } from "bun:test";
import type { Refusal } from "../../api/types/Refusal";
import { anyExplicit, draftPolicy, holderOf, policyName } from "./draft";

const row = (action: string, resource: string, decision = "implicitDeny"): Refusal => ({ principal: "arn:aws:iam::1:user/app", action, resource, decision, reason: "", count: 1, last_seen: "" });

describe("make this pass", () => {
  test("one statement per resource with its refused actions, explicit denies left out", () => {
    const doc = JSON.parse(
      draftPolicy([row("sqs:SendMessage", "arn:aws:sqs:us-east-1:1:jobs"), row("sqs:GetQueueUrl", "arn:aws:sqs:us-east-1:1:jobs"), row("sqs:ListQueues", "*"), row("s3:GetObject", "arn:aws:s3:::b/k", "explicitDeny")])!,
    ) as { Version: string; Statement: { Sid: string; Action: string[]; Resource: string; Effect: string }[] };
    expect(doc.Version).toBe("2012-10-17");
    expect(doc.Statement).toHaveLength(2);
    expect(doc.Statement[0]).toEqual({ Sid: "DozeDraftedAnyResource", Effect: "Allow", Action: ["sqs:ListQueues"], Resource: "*" });
    expect(doc.Statement[1]!.Action).toEqual(["sqs:GetQueueUrl", "sqs:SendMessage"]);
    expect(doc.Statement[1]!.Sid).toMatch(/^[A-Za-z0-9]+$/);
    expect(draftPolicy([])).toBeNull();
    expect(draftPolicy([row("s3:GetObject", "*", "explicitDeny")])).toBeNull();
    expect(anyExplicit([row("a:b", "*", "explicitDeny")])).toBe(true);
  });

  test("the user or role behind a principal, and a name for the policy", () => {
    expect(holderOf("arn:aws:iam::123:user/apps/worker")).toEqual({ kind: "user", name: "worker" });
    expect(holderOf("arn:aws:sts::123:assumed-role/report-job/s1")).toEqual({ kind: "role", name: "report-job" });
    expect(holderOf("arn:aws:iam::123:root")).toBeNull();
    expect(policyName(["sqs:ListQueues"])).toBe("doze-sqs-ListQueues");
    expect(policyName(["sqs:ListQueues", "sqs:SendMessage"])).toBe("doze-sqs-ListQueues-and-more");
  });
});
