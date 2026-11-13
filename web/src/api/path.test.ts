import { describe, expect, test } from "bun:test";
import { segment } from "./path";

const pathOf = (s: string) => new URL(`http://h${s}`).pathname;

describe("segment", () => {
  test("a URL parser collapses `.`, `..` and `%2E`: that is why a dot name needs its mark", () => {
    expect(pathOf("/api/v1/groups/./offsets/delete")).toBe("/api/v1/groups/offsets/delete");
    expect(pathOf("/api/v1/groups/%2E/offsets/delete")).toBe("/api/v1/groups/offsets/delete");
    expect(pathOf("/api/v1/groups/%2e%2E/delete")).toBe("/api/v1/delete");
  });

  test("a name of one or two dots is written with the mark and survives the parser", () => {
    expect(segment(".")).toBe("!.");
    expect(segment("..")).toBe("!..");
    // What the audit hit: a group named `.` must stay a group, not climb to another route.
    expect(pathOf(`/api/v1/groups/${segment(".")}/offsets/delete`)).toBe("/api/v1/groups/!./offsets/delete");
    expect(pathOf(`/api/v1/groups/${segment("..")}/delete`)).toBe("/api/v1/groups/!../delete");
    // A rule id from the address bar cannot climb either.
    expect(pathOf(`/api/v1/lab/rules/${segment("../../groups/x")}/remove`)).toBe("/api/v1/lab/rules/..%2F..%2Fgroups%2Fx/remove");
  });

  test("every other name is the server's base::segment, and `!` is always encoded so the mark is never ambiguous", () => {
    expect(segment("payments.settled")).toBe("payments.settled");
    expect(segment("...")).toBe("...");
    expect(segment(".hidden")).toBe(".hidden");
    expect(segment("!.")).toBe("%21.");
    expect(segment("a/b")).toBe("a%2Fb");
    expect(segment("a b?#%")).toBe("a%20b%3F%23%25");
    expect(segment("it's (x)*!")).toBe("it%27s%20%28x%29%2A%21");
    expect(segment("ü")).toBe("%C3%BC");
    expect(segment(42)).toBe("42");
  });

  test("every segment survives a URL parser as one segment", () => {
    for (const name of [".", "..", "...", "a/../b", "%2E", "!.", "x y", "ü", "~a_b-c"]) {
      const parts = pathOf(`/api/v1/groups/${segment(name)}/story`).split("/");
      expect(parts.length).toBe(6);
      const raw = parts[4];
      // The server's reading: the mark first (app::undot), then percent-decoding.
      const back = raw === "!." ? "." : raw === "!.." ? ".." : decodeURIComponent(raw);
      expect(back).toBe(name);
    }
  });
});
