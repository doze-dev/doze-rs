import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import { type Summary, type Wizard, BODY_MAX, changeSetInput, changeSetNameProblem, emptyWizard, firstFree, nameProblem, noEchoLine, tagsLine, optionsInput, optionsProblem, parameterList, parameterProblem, stackInput, summaryInput, templateProblem, templateRef, validateInput } from "./wizard-model";

const summary: Summary = { Description: "d", Parameters: [{ ParameterKey: "Stage", ParameterType: "String", DefaultValue: "prod" }, { ParameterKey: "Table", ParameterType: "String" }, { ParameterKey: "Secret", NoEcho: true }, { ParameterKey: "Mode", ParameterConstraints: { AllowedValues: ["a", "b"] }, DefaultValue: "a" }] };
const create = (p: Partial<Wizard> = {}): Wizard => ({ ...emptyWizard("create"), url: "https://b.s3.us-east-1.amazonaws.com/t.yaml", name: "search", values: { Table: "search-index", Secret: "s" }, ...p });
const update = (p: Partial<Wizard> = {}): Wizard => ({ ...emptyWizard("update", "core"), ...p });

describe("step 1", () => {
  test("names the template by its URL, its text, or the stack's own", () => {
    expect(templateRef(create())).toEqual({ TemplateURL: "https://b.s3.us-east-1.amazonaws.com/t.yaml" });
    expect(templateRef(create({ source: "file", file: { name: "t.json", text: "{}" } }))).toEqual({ TemplateBody: "{}" });
    expect(templateRef(update())).toEqual({ UsePreviousTemplate: true });
    expect(templateRef(update({ keepTemplate: false, url: "https://x/y" }))).toEqual({ TemplateURL: "https://x/y" });
  });
  test("an update that keeps its template validates nothing and reads the stack", () => {
    expect(validateInput(update())).toBeNull();
    expect(summaryInput(update())).toEqual({ StackName: "core" });
    expect(summaryInput(create())).toEqual({ TemplateURL: "https://b.s3.us-east-1.amazonaws.com/t.yaml" });
  });
  test("what is wrong with the choice", () => {
    expect(templateProblem(create({ url: "" }))).toBe("Enter the URL of the template.");
    expect(templateProblem(create({ url: "ftp://x" }))).toContain("https://");
    expect(templateProblem(create({ source: "file" }))).toBe("Choose a template file.");
    expect(templateProblem(create({ source: "file", file: { name: "t", text: "x".repeat(BODY_MAX + 1) } }))).toContain("51,200 bytes");
    expect(templateProblem(create())).toBeNull();
    expect(templateProblem(update())).toBeNull();
  });
});

describe("step 2", () => {
  test("a stack name", () => {
    expect(nameProblem("", [])).toBe("Enter a stack name.");
    expect(nameProblem("1abc", [])).toContain("starting with a letter");
    expect(nameProblem("a_b", [])).toContain("letters, digits and hyphens");
    expect(nameProblem("larder-core", ["larder-core"])).toBe("A stack named larder-core already exists in this Region.");
    expect(nameProblem("a".repeat(129), [])).toContain("128");
    expect(nameProblem("larder-search", ["x"])).toBeNull();
  });
  test("parameters", () => {
    const [stage, table, secret, mode] = summary.Parameters!;
    expect(parameterProblem(create(), stage!)).toBeNull();
    expect(parameterProblem(create({ values: {} }), table!)).toBe("Enter a value for Table.");
    expect(parameterProblem(create({ values: { Mode: "c" } }), mode!)).toContain("Choose one of a, b");
    expect(parameterProblem(update({ keep: { Secret: true } }), secret!)).toBeNull();
  });
  test("every parameter is sent; a NoEcho one an update keeps goes as UsePreviousValue", () => {
    expect(parameterList(create(), summary)).toEqual([{ ParameterKey: "Stage", ParameterValue: "prod" }, { ParameterKey: "Table", ParameterValue: "search-index" }, { ParameterKey: "Secret", ParameterValue: "s" }, { ParameterKey: "Mode", ParameterValue: "a" }]);
    expect(parameterList(update({ keep: { Secret: true }, values: { Table: "t" } }), summary)[2]).toEqual({ ParameterKey: "Secret", UsePreviousValue: true });
  });
});

describe("step 3", () => {
  test("only what was chosen goes in the call, with the rollback choice always said", () => {
    expect(optionsInput(create(), summary, true)).toEqual({ DisableRollback: false });
    const w = create({ tags: [newRow("app", "larder"), newRow("", "")], topic: " arn:aws:sns:us-east-1:111122223333:alerts ", failure: "preserve", protect: true, timeout: "30", policy: '{"Statement":[]}', role: "arn:aws:iam::1:role/r" });
    expect(optionsInput(w, summary, true)).toEqual({ Tags: [{ Key: "app", Value: "larder" }], RoleARN: "arn:aws:iam::1:role/r", NotificationARNs: ["arn:aws:sns:us-east-1:111122223333:alerts"], DisableRollback: true, StackPolicyBody: '{"Statement":[]}', TimeoutInMinutes: 30, EnableTerminationProtection: true });
    expect(optionsInput(w, summary, false)).not.toHaveProperty("TimeoutInMinutes");
    expect(optionsInput(w, summary, false)).not.toHaveProperty("EnableTerminationProtection");
  });
  test("capabilities are sent when the template needs them, and must be acknowledged", () => {
    const iam: Summary = { ...summary, Capabilities: ["CAPABILITY_IAM"] };
    expect(optionsInput(create(), iam, true).Capabilities).toEqual(["CAPABILITY_IAM"]);
    expect(optionsProblem(create(), iam)).toContain("CAPABILITY_IAM");
    expect(optionsProblem(create({ capabilities: true }), iam)).toBeNull();
  });
  test("what is wrong with the options", () => {
    expect(optionsProblem(create({ topic: "alerts" }), summary)).toContain("arn:aws:sns");
    expect(optionsProblem(create({ policy: "{" }), summary)).toBe("The stack policy is not valid JSON.");
    expect(optionsProblem(create({ timeout: "0" }), summary)).toContain("whole number");
    expect(optionsProblem(create(), summary)).toBeNull();
  });
});

describe("step 4", () => {
  test("the call that makes the stack", () => {
    const input = stackInput(create(), summary);
    expect(input.StackName).toBe("search");
    expect(input.TemplateURL).toBeDefined();
    expect((input.Parameters as unknown[]).length).toBe(4);
    expect(input.DisableRollback).toBe(false);
  });
  test("an update names its stack and sends no creation options", () => {
    const input = stackInput(update({ protect: true, timeout: "5" }), summary);
    expect(input.StackName).toBe("core");
    expect(input.UsePreviousTemplate).toBe(true);
    expect(input).not.toHaveProperty("EnableTerminationProtection");
  });
  test("a change set is of the kind of the wizard and leaves the rollback choice out", () => {
    const c = changeSetInput(create(), summary, "initial");
    expect(c.ChangeSetType).toBe("CREATE");
    expect(c.ChangeSetName).toBe("initial");
    expect(c).not.toHaveProperty("DisableRollback");
    expect(changeSetInput(update(), summary, "x").ChangeSetType).toBe("UPDATE");
  });
  test("a change set's name", () => {
    expect(changeSetNameProblem("", [])).toBe("Enter a name for the change set.");
    expect(changeSetNameProblem("a", ["a"])).toContain("already exists");
    expect(changeSetNameProblem("add-reports", ["a"])).toBeNull();
  });
  test("an update says whether its tags are the stack's own", () => {
    const was = [{ Key: "app", Value: "larder" }, { Key: "stage", Value: "prod" }];
    expect(tagsLine([{ key: "app", value: "larder" }, { key: "stage", value: "prod" }], was, "core")).toBe("These two match what core has.");
    expect(tagsLine([{ key: "app", value: "larder" }], was, "core")).toBe("1 of core's two tags is kept as it is.");
    expect(tagsLine([{ key: "app", value: "x" }], was, "core")).toBe("0 of core's two tags are kept as they are.");
    expect(tagsLine([{ key: "a", value: "b" }], [{ Key: "a", Value: "b" }], "core")).toBe("This one matches what core has.");
    expect(tagsLine([{ key: "a", value: "b" }], [], "core")).toBeNull();
  });
  test("the name offered for a change set is the first one the stack does not have", () => {
    expect(firstFree("update-review", [])).toBe("update-review");
    expect(firstFree("update-review", ["update-review"])).toBe("update-review-2");
    expect(firstFree("update-review", ["update-review", "update-review-2", "x"])).toBe("update-review-3");
  });
});

describe("what an update says of a NoEcho parameter", () => {
  test("says when it was set, in the board's words, for a stack never updated", () => {
    expect(noEchoLine("larder-core", "2026-11-06 09:00:00 UTC", false)).toBe("A NoEcho value is never shown again, so the field starts empty. Ticking Use existing value keeps the one the stack has. It was set once, when larder-core was created on 2026-11-06 09:00:00 UTC.");
  });

  test("does not say once of a stack that an update may have changed it in", () => {
    expect(noEchoLine("larder-core", "2026-11-06 09:00:00 UTC", true)).toContain("or by an update since");
    expect(noEchoLine("larder-core", undefined, false)).toBe("A NoEcho value is never shown again, so the field starts empty. Ticking Use existing value keeps the one the stack has.");
  });
});
