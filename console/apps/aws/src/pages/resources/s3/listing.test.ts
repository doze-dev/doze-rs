import { describe, expect, test } from "bun:test";
import { chosenRows, crumbsOf, folderWrong, oneObject, rowId, rowsOf, selFromText, selToText, versionRowsOf } from "./listing";

describe("S3 listings", () => {
  test("a listing is its folders, then its objects, without the folder's own marker", () => {
    const rows = rowsOf({ CommonPrefixes: [{ Prefix: "a/b/" }], Contents: [{ Key: "a/" }, { Key: "a/x.txt", Size: 3 }] }, "a/");
    expect(rows.map((r) => [r.key, r.folder])).toEqual([
      ["a/b/", true],
      ["a/x.txt", false],
    ]);
  });

  test("a versions listing has a row for every version and every delete marker, the newest of a key first", () => {
    const rows = versionRowsOf(
      {
        CommonPrefixes: [{ Prefix: "banners/" }],
        Versions: [
          { Key: "b.jpg", VersionId: "v1", LastModified: "2026-11-07T16:21:07Z", Size: 5, IsLatest: false },
          { Key: "a.jpg", VersionId: "v0", LastModified: "2026-11-07T16:21:05Z", Size: 3, IsLatest: true },
        ],
        DeleteMarkers: [{ Key: "b.jpg", VersionId: "m1", LastModified: "2026-11-08T07:58:12Z", IsLatest: true }],
      },
      "",
    );
    expect(rows.map((r) => [r.key, r.versionId, r.marker ?? false])).toEqual([
      ["banners/", undefined, false],
      ["a.jpg", "v0", false],
      ["b.jpg", "m1", true],
      ["b.jpg", "v1", false],
    ]);
    expect(rowId(rows[2]!)).toBe("b.jpg\u0000m1");
    expect(rowId(rows[0]!)).toBe("banners/");
  });

  test("a prefix is its crumbs, and a folder's name holds no slash", () => {
    expect(crumbsOf("a/b/")).toEqual([
      { label: "a", prefix: "a/" },
      { label: "a/b/".split("/")[1]!, prefix: "a/b/" },
    ]);
    expect(crumbsOf("")).toEqual([]);
    expect(folderWrong("a/b")).toContain("cannot hold");
    expect(folderWrong("banners")).toBeNull();
  });

  test("a selection is one object when it is one object, and it travels in the address", () => {
    const rows = rowsOf({ CommonPrefixes: [{ Prefix: "a/" }], Contents: [{ Key: "x.txt" }, { Key: "y.txt" }] }, "");
    expect(oneObject(chosenRows(rows, new Set(["x.txt"])))?.key).toBe("x.txt");
    expect(oneObject(chosenRows(rows, new Set(["a/"])))).toBeNull();
    expect(oneObject(chosenRows(rows, new Set(["x.txt", "y.txt"])))).toBeNull();
    const text = selToText(chosenRows(rows, new Set(["x.txt", "a/"])));
    expect(selFromText(text)).toEqual([{ key: "a/" }, { key: "x.txt" }]);
    expect(selFromText("not json")).toEqual([]);
    const versions = versionRowsOf({ Versions: [{ Key: "k", VersionId: "v1" }] }, "");
    expect(selFromText(selToText(versions))).toEqual([{ key: "k", versionId: "v1" }]);
  });
});
