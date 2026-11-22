import { describe, expect, test } from "bun:test";
import { groupWords } from "./delete-entity";

const blocker = (title: string, names: string[]) => ({ title, items: names.map((name) => ({ name })), more: false }) as unknown as Parameters<typeof groupWords>[1];

describe("what a delete says of its dependents", () => {
  test("the groups of a user stay, in either number", () => {
    expect(groupWords("dev", blocker("Groups", ["developers"]))).toEqual({ lead: "dev will be removed from 1 group.", rest: "developers: the group stays." });
    expect(groupWords("dev", blocker("Groups", ["developers", "ops"]))).toEqual({ lead: "dev will be removed from 2 groups.", rest: "developers and ops: the groups stay." });
  });
});
