import { describe, expect, test } from "bun:test";
import type { Refusal } from "../../api/types/Refusal";
import { addTo, allows, covering, widen, wild } from "./merge";

const readonly = JSON.stringify({ Version: "2012-10-17", Statement: [{ Sid: "Read", Effect: "Allow", Action: ["s3:GetObject", "s3:ListBucket"], Resource: ["arn:aws:s3:::reports/*", "arn:aws:s3:::reports"] }] });
const row = (action: string, resource: string, decision = "implicitDeny"): Refusal => ({ principal: "arn:aws:iam::1:user/report-job", action, resource, decision, reason: "because", count: 1 }) as Refusal;

describe("wild", () => {
  test("* and ? as IAM reads them", () => {
    expect(wild("arn:aws:s3:::reports/*", "arn:aws:s3:::reports/2026/a.csv")).toBe(true);
    expect(wild("arn:aws:s3:::reports/*", "arn:aws:s3:::receipts/r.pdf")).toBe(false);
    expect(wild("s3:Get*", "s3:getobject", true)).toBe(true);
    expect(wild("a?c", "abc")).toBe(true);
    expect(wild("a.c", "abc")).toBe(false);
  });
});

describe("covering", () => {
  test("counts the statements that allow the action on the resource", () => {
    expect(covering(readonly, "s3:GetObject", "arn:aws:s3:::reports/a")).toBe(1);
    expect(covering(readonly, "s3:GetObject", "arn:aws:s3:::receipts/a")).toBe(0);
    expect(covering(null, "s3:GetObject", "x")).toBe(0);
    expect(covering("not json", "s3:GetObject", "x")).toBe(0);
  });
  test("a deny, a condition or a Not* is not an allow this can count", () => {
    expect(allows({ Effect: "Deny", Action: "*", Resource: "*" }, "a:B", "r")).toBe(false);
    expect(allows({ Effect: "Allow", Action: "*", Resource: "*", Condition: {} }, "a:B", "r")).toBe(false);
    expect(allows({ Effect: "Allow", NotAction: "a:B", Resource: "*" }, "a:C", "r")).toBe(false);
  });
});

describe("addTo", () => {
  test("widens an object to its bucket's objects and adds it to the existing statements", () => {
    expect(widen("arn:aws:s3:::receipts/2026-11-08/r-8841.pdf")).toBe("arn:aws:s3:::receipts/*");
    expect(widen("arn:aws:sqs:us-east-1:1:q")).toBe("arn:aws:sqs:us-east-1:1:q");
    const r = addTo(readonly, [row("s3:GetObject", "arn:aws:s3:::receipts/2026-11-08/r-8841.pdf")]);
    expect(r.added).toEqual(["s3:GetObject on arn:aws:s3:::receipts/*"]);
    const doc = JSON.parse(r.document) as { Statement: { Sid: string; Resource: unknown }[] };
    expect(doc.Statement.length).toBe(2);
    expect(doc.Statement[0]?.Sid).toBe("Read");
    expect(doc.Statement[1]).toMatchObject({ Sid: "AllowGetObjectreceipts", Effect: "Allow", Action: "s3:GetObject", Resource: "arn:aws:s3:::receipts/*" });
  });
  test("adds nothing the policy already covers, and nothing an explicit deny refused", () => {
    expect(addTo(readonly, [row("s3:GetObject", "arn:aws:s3:::reports/a.csv")]).added).toEqual([]);
    expect(addTo(readonly, [row("s3:GetObject", "arn:aws:s3:::receipts/a", "explicitDeny")]).added).toEqual([]);
  });
  test("actions on one resource go in one statement; a Sid in use is made apart", () => {
    const r = addTo(readonly, [row("s3:GetObject", "arn:aws:s3:::receipts/a"), row("s3:PutObject", "arn:aws:s3:::receipts/b")]);
    expect(r.added).toEqual(["s3:GetObject, s3:PutObject on arn:aws:s3:::receipts/*"]);
    const twice = addTo(r.document, [row("sqs:SendMessage", "arn:aws:sqs:us-east-1:1:receipts")]);
    expect(new Set((JSON.parse(twice.document) as { Statement: { Sid: string }[] }).Statement.map((s) => s.Sid)).size).toBe(3);
  });
});
