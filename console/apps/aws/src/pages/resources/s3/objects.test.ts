import { describe, expect, test } from "bun:test";
import { crumbsOf, rowsOf } from "./objects";
import { folderWrong } from "./dialogs";

describe("S3 objects", () => {
  test("a listing is its folders, then its objects, without the folder's own marker", () => {
    const rows = rowsOf({ CommonPrefixes: [{ Prefix: "a/b/" }], Contents: [{ Key: "a/" }, { Key: "a/x.txt", Size: 3 }] }, "a/");
    expect(rows.map((r) => [r.key, r.folder])).toEqual([
      ["a/b/", true],
      ["a/x.txt", false],
    ]);
  });

  test("a prefix is its crumbs, and a folder's name holds no slash", () => {
    expect(crumbsOf("a/b/")).toEqual([
      { label: "a", prefix: "a/" },
      { label: "b", prefix: "a/b/" },
    ]);
    expect(crumbsOf("")).toEqual([]);
    expect(folderWrong("a/b")).toContain("cannot hold");
    expect(folderWrong("banners")).toBeNull();
  });
});
