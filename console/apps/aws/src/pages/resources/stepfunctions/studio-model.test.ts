import { describe, expect, test } from "bun:test";
import { type Validation, encryptionOf, encryptionProblem, furtherProblems, lineOfLocation, problemLines, wordsOf } from "./studio-model";

const text = JSON.stringify({ StartAt: "A", States: { A: { Type: "Pass", Next: "Nowhere" } } }, null, 2);
const dangling: Validation = {
  result: "FAIL",
  diagnostics: [
    { severity: "ERROR", code: "MISSING_TRANSITION_TARGET", message: "Missing 'Next' target: Nowhere", location: "/States/A/Next" },
    { severity: "ERROR", code: "MISSING_END_STATE", message: "Workflow has no terminal state" },
  ],
};

describe("what a bad definition says", () => {
  test("a transition to a state that is not there says which state points where", () => {
    expect(wordsOf(dangling.diagnostics[0]!)).toBe("The definition below points A at a state that doesn't exist: Next names Nowhere.");
  });
  test("the missing end state that only follows from it is not listed, and the first problem is the sentence, not the list", () => {
    expect(furtherProblems(dangling)).toEqual([]);
  });
  test("a missing end state with nothing else wrong is a problem of its own", () => {
    const v: Validation = { result: "FAIL", diagnostics: [dangling.diagnostics[1]!, { severity: "ERROR", code: "SCHEMA_VALIDATION_FAILED", message: "bad", location: "/States/A" }] };
    expect(furtherProblems(v).map((d) => d.code)).toEqual(["SCHEMA_VALIDATION_FAILED"]);
  });
  test("the line the error names is the one drawn in the link colour", () => {
    expect(problemLines(text, dangling)).toEqual([text.split("\n").findIndex((r) => r.includes('"Next"')) + 1]);
    expect(problemLines(text, undefined)).toEqual([]);
  });
});

describe("encryption of an activity or a machine", () => {
  test("nothing is sent while the switch is off; the call carries the key and the reuse period when it is on", () => {
    expect(encryptionOf({ kms: false, kmsKey: "k", kmsPeriod: "300" })).toEqual({});
    expect(encryptionOf({ kms: true, kmsKey: "k", kmsPeriod: "300" })).toEqual({ encryptionConfiguration: { type: "CUSTOMER_MANAGED_KMS_KEY", kmsKeyId: "k", kmsDataKeyReusePeriodSeconds: 300 } });
  });
  test("a key and a period of 60 to 900 seconds are what is missing", () => {
    expect(encryptionProblem({ kms: false, kmsKey: null, kmsPeriod: "" })).toBeNull();
    expect(encryptionProblem({ kms: true, kmsKey: null, kmsPeriod: "300" })).toBe("Choose a KMS key.");
    expect(encryptionProblem({ kms: true, kmsKey: "k", kmsPeriod: "30" })).toBe("The data key reuse period is 60 to 900 seconds.");
    expect(encryptionProblem({ kms: true, kmsKey: "k", kmsPeriod: "900" })).toBeNull();
  });
});

describe("the line an error names", () => {
  test("is the key's own line, not an earlier line that only names it as a value", () => {
    // `Next: "QueueReorder"` comes before the QueueReorder state: the error is on the Next inside it.
    const def = { StartAt: "C", States: { C: { Type: "Choice", Choices: [{ Next: "QueueReorder" }], Default: "S" }, QueueReorder: { Type: "Task", Next: "Notify" }, S: { Type: "Succeed" } } };
    const pretty = JSON.stringify(def, null, 2);
    const rows = pretty.split("\n");
    const want = rows.findIndex((r) => r.includes('"Next": "Notify"')) + 1;
    expect(lineOfLocation(pretty, "/States/QueueReorder/Next")).toBe(want);
    expect(lineOfLocation(pretty, "/States/Nope/Next")).toBe(rows.findIndex((r) => r.includes('"States"')) + 1);
    expect(lineOfLocation(pretty, "null")).toBeNull();
  });
});

describe("the encryption of a call that has no key yet", () => {
  test("names the key as a placeholder, not as null", () => {
    expect(encryptionOf({ kms: true, kmsKey: null, kmsPeriod: "300" })).toEqual({ encryptionConfiguration: { type: "CUSTOMER_MANAGED_KMS_KEY", kmsKeyId: "<KMS key>", kmsDataKeyReusePeriodSeconds: 300 } });
    expect(encryptionProblem({ kms: true, kmsKey: null, kmsPeriod: "300" })).toBe("Choose a KMS key.");
  });
});
