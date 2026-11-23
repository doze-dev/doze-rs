import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import type { ColumnSpec } from "../../../api/types/ColumnSpec";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { blankSecret, cadenceOf, cadenceProblem, dayOf, expressionOf, durationProblem, functionName, listSentence, nameProblem, replicaProblem, replicaRegions, rotationSentence, rowsValue, savingWords, scheduleProblem, scheduleWords, secretCalls, secretRows, secretValue, tokenWords, tokensFromQuery, tokensToQuery, valueProblem, valueRows, versions, withStage } from "./model";

const col = (title: string): ColumnSpec => ({ title, format: "text", badge: null });
const COLUMNS = ["Description", "Rotation", "Last changed", "Last retrieved", "Marked for deletion", "Encryption key"].map(col);
const row = (name: string, rotation: string, deleted: string | null = null): ResourceRow => ({ name, label: name, id: name, cells: ["d", rotation, null, null, deleted, "alias/larder-app"], link: null });

describe("the list", () => {
  test("search tokens go to the address and back, and what is not a token is dropped", () => {
    const tokens = [{ prop: "name" as const, text: "larder/prod/" }, { prop: "tag-key" as const, text: "app" }];
    expect(tokensToQuery(tokens)).toBe("name:larder/prod/|tag-key:app");
    expect(tokensFromQuery("name:larder/prod/|tag-key:app")).toEqual(tokens);
    expect(tokensFromQuery("nope:x|name:|justtext")).toEqual([]);
    expect(tokensToQuery([{ prop: "all", text: "a|b" }])).toBe("all:a b");
    expect(tokenWords(tokens[0]!)).toBe("Name: larder/prod/");
  });
  test("the sentence counts the secrets and what some of them have", () => {
    const rows = secretRows([row("a", "On"), row("b", "Off", "1"), row("c", "Off")], COLUMNS);
    expect(listSentence(rows, false, null)).toBe("3 secrets: 1 with rotation on and 1 scheduled for deletion.");
    expect(listSentence(rows.slice(2), false, null)).toBe("1 secret.");
    expect(listSentence(rows.slice(0, 2), true, 8)).toBe("2 secrets match of 8.");
    expect(listSentence(rows.slice(0, 1), true, null)).toBe("1 secret matches.");
  });
});

describe("the value", () => {
  test("a JSON object of simple values is rows, anything else is plaintext", () => {
    expect(valueRows('{"secret_key":"sk","n":2,"ok":true}')).toEqual([{ key: "secret_key", value: "sk" }, { key: "n", value: "2" }, { key: "ok", value: "true" }]);
    expect(valueRows("sk_live_plain")).toBeNull();
    expect(valueRows('{"a":{"b":1}}')).toBeNull();
    expect(valueRows("[1]")).toBeNull();
    expect(valueRows("{}")).toBeNull();
  });
  test("rows are stored as one JSON object, blank rows dropped", () => {
    expect(rowsValue([newRow("a", "1"), newRow("", ""), newRow("b", "")])).toBe('{"a":"1","b":""}');
    expect(valueProblem("")).toBe("Enter a value.");
    expect(valueProblem("x".repeat(65537))).toMatch(/65,536/);
    expect(valueProblem("ok")).toBeNull();
  });
});

describe("versions and rotation", () => {
  const vs = versions([{ VersionId: "e41b6d08-27a3", VersionStages: ["AWSPREVIOUS"], CreatedDate: 1000 }, { VersionId: "9c2e7b1a-5d4f", VersionStages: ["AWSCURRENT"], CreatedDate: 2000 }]);
  test("versions are newest first and found by label", () => {
    expect(vs.map((v) => v.id.slice(0, 4))).toEqual(["9c2e", "e41b"]);
    expect(withStage(vs, "AWSCURRENT")?.id).toBe("9c2e7b1a-5d4f");
    expect(savingWords(vs)).toBe("Today that means 9c2e7b1a becomes AWSPREVIOUS and e41b6d08 loses its label.");
    expect(savingWords([])).toBe("Saving creates a version labeled AWSCURRENT.");
  });
  test("rotation is said by who last rotated and when it rotates next", () => {
    const d = { RotationEnabled: true, RotationLambdaARN: "arn:aws:lambda:us-east-1:1:function:rotate-stripe-key", LastRotatedDate: Date.UTC(2026, 10, 6) / 1000, NextRotationDate: Date.UTC(2026, 11, 6) / 1000, RotationRules: { ScheduleExpression: "rate(30 days)" } };
    expect(rotationSentence(d)).toBe("rotate-stripe-key last rotated it on Nov 6, 2026 and rotates it next on Dec 6, 2026.");
    expect(rotationSentence({ ...d, LastRotatedDate: undefined, NextRotationDate: undefined })).toBe("Rotation is on with rotate-stripe-key and has not run yet.");
    expect(rotationSentence({ RotationEnabled: false })).toBeNull();
    expect(scheduleWords(d)).toBe("rate(30 days)");
    expect(scheduleWords({ RotationRules: { AutomaticallyAfterDays: 7 } })).toBe("every 7 days");
    expect(functionName("arn:aws:lambda:us-east-1:1:function:f")).toBe("f");
    expect(dayOf(null)).toBeNull();
    expect(dayOf(Date.UTC(2026, 10, 6) / 1000)).toBe("Nov 6, 2026");
  });
  test("a schedule is rate or cron and a window is in hours", () => {
    expect(scheduleProblem("rate(30 days)")).toBeNull();
    expect(scheduleProblem("every month")).toMatch(/rate/);
    expect(durationProblem("3h")).toBeNull();
    expect(durationProblem("")).toBeNull();
    expect(durationProblem("90m")).toMatch(/hours/);
  });
});

describe("the wizard", () => {
  test("a name is made of what the service takes and is not taken", () => {
    expect(nameProblem("", [])).toBe("Enter a name.");
    expect(nameProblem("has space", [])).toMatch(/letters/);
    expect(nameProblem("a", ["a"])).toMatch(/already exists/);
    expect(nameProblem("larder/prod/x", ["a"])).toBeNull();
  });
  test("Store makes CreateSecret, then the policy and the rotation when there are some, the value masked for the preview", () => {
    const f = { ...blankSecret(), name: "larder/prod/search-key", description: "Search provider API key.", pairs: [newRow("api_key", "x")], key: "alias/larder-app", tags: [newRow("app", "search-indexer")] };
    const calls = secretCalls(f, { masked: true });
    expect(calls).toHaveLength(1);
    expect(calls[0]!.input).toEqual({ Name: "larder/prod/search-key", Description: "Search provider API key.", SecretString: "(masked)", KmsKeyId: "alias/larder-app", Tags: [{ Key: "app", Value: "search-indexer" }] });
    expect(secretValue(f)).toBe('{"api_key":"x"}');
    const more = secretCalls({ ...f, policy: "{}", rotate: true, fn: "arn:aws:lambda:us-east-1:1:function:rot", duration: "3h" }, { masked: false });
    expect(more.map((c) => c.operation)).toEqual(["CreateSecret", "PutResourcePolicy", "RotateSecret"]);
    expect(more[2]!.input).toEqual({ SecretId: "larder/prod/search-key", RotationLambdaARN: "arn:aws:lambda:us-east-1:1:function:rot", RotationRules: { ScheduleExpression: "rate(30 days)", Duration: "3h" }, RotateImmediately: false });
    expect(secretValue({ ...f, mode: "text", text: "plain" })).toBe("plain");
  });
});

describe("the rotation schedule builder", () => {
  test("a rate in days or hours is a cadence, anything else is an expression only", () => {
    expect(cadenceOf("rate(30 days)")).toEqual({ n: "30", unit: "days" });
    expect(cadenceOf("rate(1 day)")).toEqual({ n: "1", unit: "days" });
    expect(cadenceOf("rate(4 hours)")).toEqual({ n: "4", unit: "hours" });
    expect(cadenceOf("cron(0 4 1 * ? *)")).toBeNull();
    expect(cadenceOf("rate(300 minutes)")).toBeNull();
  });
  test("a cadence is written as AWS writes it", () => {
    expect(expressionOf({ n: "30", unit: "days" })).toBe("rate(30 days)");
    expect(expressionOf({ n: "1", unit: "days" })).toBe("rate(1 day)");
    expect(expressionOf({ n: "6", unit: "hours" })).toBe("rate(6 hours)");
  });
  test("days are 1 to 1,000 and hours 4 or more", () => {
    expect(cadenceProblem({ n: "30", unit: "days" })).toBeNull();
    expect(cadenceProblem({ n: "0", unit: "days" })).toMatch(/1 to 1,000/);
    expect(cadenceProblem({ n: "3", unit: "hours" })).toMatch(/every 4 hours/);
    expect(cadenceProblem({ n: "x", unit: "days" })).toBe("Enter a whole number.");
  });
});

describe("replicas in the create form", () => {
  test("are Region names, not the Region the secret is in, each once; a blank row is dropped", () => {
    const all = ["eu-west-1", "", "eu-west-1", "us-east-1", "nope"];
    expect(replicaProblem(all[0]!, 0, all, "us-east-1")).toBeNull();
    expect(replicaProblem(all[1]!, 1, all, "us-east-1")).toBeNull();
    expect(replicaProblem(all[2]!, 2, all, "us-east-1")).toBe("Another row has this Region.");
    expect(replicaProblem(all[3]!, 3, all, "us-east-1")).toBe("The secret is already in us-east-1.");
    expect(replicaProblem(all[4]!, 4, all, "us-east-1")).toMatch(/Region name/);
    expect(replicaRegions(all)).toEqual(["eu-west-1", "eu-west-1", "us-east-1", "nope"]);
  });
  test("CreateSecret carries them as AddReplicaRegions", () => {
    const f = { ...blankSecret(), name: "n", pairs: [newRow("k", "v")], replicas: ["eu-west-1", " "] };
    expect((secretCalls(f, { masked: true })[0]!.input as Record<string, unknown>).AddReplicaRegions).toEqual([{ Region: "eu-west-1" }]);
    expect((secretCalls(blankSecret(), { masked: true })[0]!.input as Record<string, unknown>).AddReplicaRegions).toBeUndefined();
  });
});
