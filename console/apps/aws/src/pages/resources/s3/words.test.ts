import { describe, expect, test } from "bun:test";
import { baseName, copySource, longDate, nameProblems, nameRefusal, parentPrefix, regionWords, shortDate, size, suggestName, uploadKey, versioningOf, whenMs } from "./words";

describe("S3 words", () => {
  test("a Region and a time are written as the AWS console writes them", () => {
    expect(regionWords("us-east-1")).toBe("US East (N. Virginia) us-east-1");
    expect(regionWords("zz-nowhere-9")).toBe("zz-nowhere-9");
    expect(longDate(Date.UTC(2026, 10, 8, 17, 22, 10) / 1000)).toBe("November 8, 2026, 17:22:10 (UTC+00:00)");
    expect(shortDate("2026-11-07T16:21:05Z")).toBe("Nov 7, 2026, 16:21:05 (UTC+00:00)");
    expect(longDate(undefined)).toBe("-");
    expect(whenMs("1794075730000")).toBe(1794075730000);
  });

  test("a size is to the kilobyte, one decimal", () => {
    expect(size(0)).toBe("0 B");
    expect(size(212 * 1024)).toBe("212.0 KB");
    expect(size(38.6 * 1024 * 1024)).toBe("38.6 MB");
    expect(size(undefined)).toBe("-");
  });

  test("versioning is off for a bucket that never had it", () => {
    expect(versioningOf(undefined)).toBe("Disabled");
    expect(versioningOf("Suspended")).toBe("Suspended");
  });

  test("a name is checked against the naming rules and a better one is offered", () => {
    expect(nameProblems("larder-exports")).toEqual([]);
    expect(nameProblems("Larder_Exports")).toEqual(["has an uppercase letter and an underscore"]);
    expect(nameProblems("ab")).toContain("is shorter than 3 characters");
    expect(nameProblems("a..b")).toContain("has two periods in a row");
    expect(nameProblems("-abc")).toContain("does not begin with a letter or a number");
    expect(suggestName("Larder_Exports")).toBe("larder-exports");
    expect(nameRefusal("Larder_Exports")).toEqual({ why: "Larder_Exports has an uppercase letter and an underscore, which the naming rules do not allow.", suggestion: "Change the name, for example to larder-exports." });
    expect(nameRefusal("larder-exports")).toBeNull();
  });

  test("a copy's source is encoded as S3 reads it", () => {
    expect(copySource("b", "a b/c.txt")).toBe("b/a%20b/c.txt");
    expect(copySource("b", "k", "v 1")).toBe("b/k?versionId=v%201");
  });

  test("an upload's key is the folder it is put in plus the path inside the folder chosen", () => {
    expect(uploadKey("banners/", { name: "hero.jpg" })).toBe("banners/hero.jpg");
    expect(uploadKey("", { name: "hero.jpg", webkitRelativePath: "autumn/hero.jpg" })).toBe("autumn/hero.jpg");
    expect(baseName("a/b/c.txt")).toBe("c.txt");
    expect(baseName("a/b/")).toBe("b");
    expect(parentPrefix("a/b/")).toBe("a/");
    expect(parentPrefix("a/")).toBe("");
  });
});
