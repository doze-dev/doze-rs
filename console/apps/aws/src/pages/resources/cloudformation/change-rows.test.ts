import { describe, expect, test } from "bun:test";
import { changeRows } from "./change-rows";

const c = (Action: string, LogicalResourceId: string) => ({ ResourceChange: { Action, LogicalResourceId, ResourceType: "AWS::SQS::Queue" } });

describe("the rows of a change set", () => {
  test("Add, then Modify, then Remove, each in the service's order", () => {
    const rows = changeRows({ Changes: [c("Modify", "A"), c("Remove", "B"), c("Add", "C"), c("Modify", "D"), c("Add", "E"), c("Dynamic", "F")] });
    expect(rows.map((r) => `${r.Action} ${r.LogicalResourceId}`)).toEqual(["Add C", "Add E", "Modify A", "Modify D", "Remove B", "Dynamic F"]);
  });
  test("a change with no ResourceChange is not a row", () => {
    expect(changeRows({ Changes: [{}, c("Add", "A")] })).toHaveLength(1);
    expect(changeRows({})).toEqual([]);
  });
});
