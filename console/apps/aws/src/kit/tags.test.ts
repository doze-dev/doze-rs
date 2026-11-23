import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import { diffTags, noChange, tagWords } from "./tags";

const before = [{ key: "app", value: "fulfilment" }, { key: "env", value: "dev" }];

describe("diffTags", () => {
  test("no edit is no change", () => expect(noChange(diffTags(before, before.map((t) => newRow(t.key, t.value))))).toBe(true));
  test("a new tag and a new value are set; a tag whose row is gone is removed", () => {
    const rows = [newRow("app", "fulfilment"), newRow("env", "prod"), newRow("team", "ops")];
    expect(diffTags(before, rows)).toEqual({ set: [{ key: "env", value: "prod" }, { key: "team", value: "ops" }], remove: [] });
    expect(diffTags(before, [newRow("app", "fulfilment")])).toEqual({ set: [], remove: ["env"] });
  });
  test("a renamed key is a remove and a set; blank rows are ignored", () => {
    expect(diffTags(before, [newRow("app", "fulfilment"), newRow("environment", "dev"), newRow()])).toEqual({ set: [{ key: "environment", value: "dev" }], remove: ["env"] });
  });
  test("words", () => {
    expect(tagWords(1)).toBe("1 tag");
    expect(tagWords(3, "cost allocation tag")).toBe("3 cost allocation tags");
  });
});

import { changeWords } from "./tags";
describe("changeWords", () => {
  test("says what a save does", () => {
    expect(changeWords({ set: [], remove: [] }, before)).toBe("Changes nothing.");
    expect(changeWords({ set: [{ key: "team", value: "ops" }], remove: [] }, before)).toBe("Adds 1 tag.");
    expect(changeWords({ set: [{ key: "env", value: "prod" }, { key: "team", value: "ops" }], remove: ["app"] }, before)).toBe("Adds 1 tag, changes 1 tag and removes 1 tag.");
  });
});
