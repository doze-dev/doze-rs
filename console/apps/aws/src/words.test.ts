import { expect, test } from "bun:test";
import { authWords, iamWords, OUTCOMES, outcomeSentence, serviceTitle } from "./words";

test("each IAM mode has its chip and its sentence, and an unknown one is shown as it is", () => {
  expect(iamWords("off").chip).toBe("IAM off");
  expect(iamWords("soft").sentence).toContain("logged and let through");
  expect(iamWords("enforce").sentence).toContain("AccessDenied");
  expect(iamWords("strict")).toEqual({ chip: "IAM strict", sentence: "IAM mode: strict." });
});

test("each auth mode says whether signatures are checked", () => {
  expect(authWords("trust")).toContain("not checked");
  expect(authWords("verify")).toContain("Every signature is checked");
  expect(authWords("other")).toBe("Auth mode: other.");
});

test("the four outcomes of the contract each have a word and a tone, and no two share a tone", () => {
  expect(Object.keys(OUTCOMES).sort()).toEqual(["denied", "error", "ok", "refused"]);
  expect(new Set(Object.values(OUTCOMES).map((o) => o.tone)).size).toBe(4);
  expect(OUTCOMES.refused.label).toBe("REFUSED");
  expect(OUTCOMES.denied.label).toBe("DENIED");
});

test("why a call ended as it did, by what the contract says of each outcome", () => {
  expect(outcomeSentence({ outcome: "ok", status: 200, error_code: null })).toBe("Answered as asked.");
  expect(outcomeSentence({ outcome: "denied", status: 403, error_code: "AccessDenied" })).toContain("IAM said no");
  expect(outcomeSentence({ outcome: "refused", status: 501, error_code: "NotImplemented" })).toContain("not built");
  expect(outcomeSentence({ outcome: "refused", status: 403, error_code: "OptInRequired" })).toContain("--services");
  expect(outcomeSentence({ outcome: "error", status: 400, error_code: "QueueDoesNotExist" })).toContain("caller's to fix");
  expect(outcomeSentence({ outcome: "error", status: 500, error_code: "InternalFailure" })).toContain("error of its own");
});

test("a service is named by its title when the build has it, else by the name the call carried", () => {
  const titles = new Map([["stepfunctions", "Step Functions"]]);
  expect(serviceTitle("stepfunctions", titles)).toBe("Step Functions");
  expect(serviceTitle("bedrock", titles)).toBe("bedrock");
  expect(serviceTitle(null, titles)).toBeNull();
});
