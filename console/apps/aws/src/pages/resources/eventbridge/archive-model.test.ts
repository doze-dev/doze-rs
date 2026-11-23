import { describe, expect, test } from "bun:test";
import { archiveProblem, createArchiveInput, daysProblem, emptyArchive, nameProblem } from "./archive-model";

describe("the create-archive form", () => {
  const f = { ...emptyArchive("larder"), name: "orders-30d" };
  test("the name and the days", () => {
    expect(nameProblem("orders-30d")).toBeNull();
    expect(nameProblem("")).toBe("Name the archive.");
    expect(nameProblem("x".repeat(49))).toContain("48");
    expect(nameProblem("a b")).toContain("letters");
    expect(daysProblem({ indefinite: true, days: "x" })).toBeNull();
    expect(daysProblem({ indefinite: false, days: "0" })).toContain("1 or more");
    expect(daysProblem({ indefinite: false, days: "30" })).toBeNull();
  });
  test("a taken name, a bad pattern", () => {
    expect(archiveProblem(f, ["orders-30d"])).toBe("orders-30d is already an archive.");
    expect(archiveProblem({ ...f, filter: true, pattern: "{" }, [])).toContain("Not JSON");
    expect(archiveProblem(f, [])).toBeNull();
  });
  test("CreateArchive sends what was set: no retention is indefinite, no filter is every event", () => {
    expect(createArchiveInput(f, "arn:b")).toEqual({ ArchiveName: "orders-30d", EventSourceArn: "arn:b" });
    expect(createArchiveInput({ ...f, description: "d", indefinite: false, days: "30", filter: true }, "arn:b")).toEqual({ ArchiveName: "orders-30d", EventSourceArn: "arn:b", Description: "d", RetentionDays: 30, EventPattern: '{"source":["larder.orders"]}' });
  });
});
