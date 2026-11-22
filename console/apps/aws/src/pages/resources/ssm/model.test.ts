import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import type { ColumnSpec } from "../../../api/types/ColumnSpec";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { blankParam, filterSentence, labelElsewhere, labelProblem, listSentence, moveWords, nameProblem, paramRows, pasts, putInput, tokensFromQuery, tokensToQuery, valueProblem, versionLine, whenWords } from "./model";

const col = (title: string): ColumnSpec => ({ title, format: "text", badge: null });
const COLUMNS = ["Type", "Tier", "Version", "Last modified", "Description", "KMS key"].map(col);
const row = (name: string, type: string, tier: string, version: string, at: string): ResourceRow => ({ name, label: name, id: name, cells: [type, tier, version, at, "d", null], link: null });
const rows = paramRows([row("/a", "String", "Standard", "1", "2026-11-06T19:02:00Z"), row("/b", "SecureString", "Standard", "3", "2026-11-07T10:22:00Z"), row("/c", "String", "Advanced", "2", "2026-11-07T17:05:00Z")], COLUMNS);

describe("the list", () => {
  test("filter tokens go to the address and back; a type always compares by equality", () => {
    const tokens = [{ prop: "Name", option: "BeginsWith", text: "/larder/staging" }, { prop: "Type", text: "String" }];
    expect(tokensToQuery(tokens)).toBe("Name:BeginsWith:/larder/staging|Type:Equals:String");
    expect(tokensFromQuery("Name:BeginsWith:/larder/staging|Type:Equals:String")).toEqual([{ prop: "Name", option: "BeginsWith", text: "/larder/staging" }, { prop: "Type", text: "String" }]);
    expect(tokensFromQuery("Nope:Equals:x|Name:BeginsWith:")).toEqual([]);
    expect(tokensFromQuery("Name:Contains:a:b")).toEqual([{ prop: "Name", option: "Contains", text: "a:b" }]);
  });
  test("the sentence counts the parameters by tier, names the only SecureString and the newest change", () => {
    expect(listSentence(rows, "us-east-1")).toBe("There are 3 parameters in us-east-1: 2 Standard and 1 Advanced. /b is the only SecureString. The newest change is /c, saved as version 2 on Nov 7, 2026 at 17:05.");
    expect(listSentence(rows.slice(0, 1), "us-east-1")).toBe("There is 1 parameter in us-east-1: 1 Standard. The newest change is /a, saved as version 1 on Nov 6, 2026 at 19:02.");
  });
  test("the filter sentence says how many match and what they have in common", () => {
    expect(filterSentence([rows[0]!, rows[2]!], 8)).toBe("The filter matches 2 of 8 parameters.");
    expect(filterSentence([rows[0]!], 8)).toBe("The filter matches 1 of 8 parameters, a Standard String parameter.");
    expect(filterSentence([], 6)).toBe("Nothing matches among the 6 parameters.");
    expect(whenWords("junk")).toBe("junk");
  });
});

describe("the form", () => {
  test("a name has levels of simple characters and is not reserved", () => {
    expect(nameProblem("")).toBe("Enter a name.");
    expect(nameProblem("/larder/prod/db/password")).toBeNull();
    expect(nameProblem("/has space")).toMatch(/letters/);
    expect(nameProblem("/aws/x")).toMatch(/reserved/);
    expect(nameProblem("ssm-thing")).toMatch(/reserved/);
    expect(nameProblem("/" + Array.from({ length: 16 }, () => "a").join("/"))).toMatch(/15 levels/);
  });
  test("a value fits its tier and holds no template", () => {
    expect(valueProblem("", "Standard")).toBe("Enter a value.");
    expect(valueProblem("{{ssm:x}}", "Standard")).toMatch(/\{\{/);
    expect(valueProblem("x".repeat(4097), "Standard")).toMatch(/Choose Advanced/);
    expect(valueProblem("x".repeat(4097), "Advanced")).toBeNull();
    expect(valueProblem("x".repeat(8193), "Advanced")).toMatch(/8,192/);
  });
  test("PutParameter carries what is set; Overwrite only when editing; tags only on a create", () => {
    const f = { ...blankParam(), name: "/larder/prod/db/password", type: "SecureString" as const, key: "alias/larder-app", value: "v", description: "d", tags: [newRow("app", "larder")] };
    expect(putInput(f)).toEqual({ Name: "/larder/prod/db/password", Description: "d", Type: "SecureString", KeyId: "alias/larder-app", Value: "v", Tier: "Standard", DataType: "text", Tags: [{ Key: "app", Value: "larder" }] });
    expect(putInput(f, { overwrite: true, editing: true }).Tags).toBeUndefined();
    expect(putInput(f, { overwrite: true, editing: true }).Overwrite).toBe(true);
    expect(putInput({ ...f, type: "String" }).KeyId).toBeUndefined();
  });
});

describe("history and labels", () => {
  const ps = pasts([{ Version: 1, LastModifiedDate: 1, Labels: [] }, { Version: 3, LastModifiedDate: 3, Labels: ["canary"], Description: "x" }, { Version: 2, LastModifiedDate: 2, Labels: ["current"] }]);
  test("versions are newest first", () => expect(ps.map((p) => p.version)).toEqual([3, 2, 1]));
  test("a label has rules, and one that sits on another version is moved", () => {
    expect(labelProblem("")).toBe("Enter a label.");
    expect(labelProblem("1st")).toMatch(/number/);
    expect(labelProblem("aws-x")).toMatch(/aws or ssm/);
    expect(labelProblem("bad label")).toMatch(/letters/);
    expect(labelProblem("canary")).toBeNull();
    expect(labelElsewhere(ps, "current", 3)).toBe(2);
    expect(labelElsewhere(ps, "current", 2)).toBeNull();
    expect(moveWords(ps, ["current"], 3)).toBe("current is on version 2 now. Saving moves it to version 3, because a label can sit on only one version of a parameter.");
    expect(moveWords(ps, ["new"], 3)).toBeNull();
    expect(versionLine("3", "2026-11-07T10:22:00Z")).toBe("Version 3, last modified Nov 7, 2026 at 10:22 UTC.");
  });
});
