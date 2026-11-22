import { describe, expect, test } from "bun:test";
import { baseName, copySource, longDate, nameProblems, nameRefusal, parentPrefix, regionWords, shortDate, size, suggestName, uploadKey, versioningOf, whenMs, wiredChip, callKey, storageWords } from "./words";

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

test("a Wired to chip names the destination with its kind in brackets", () => {
  expect(wiredChip("Notifies Lambda function", "resize-product-image")).toBe("resize-product-image (Lambda)");
  expect(wiredChip("Notifies SQS queue", "image-events")).toBe("image-events (SQS)");
  expect(wiredChip("Notifies SNS topic", "t")).toBe("t (SNS)");
  expect(wiredChip("Something else", "x")).toBe("x");
});

test("the key a recorded call acted on is the rest of its resource after the bucket", () => {
  expect(callKey("b", "b/sku-1.jpg")).toBe("sku-1.jpg");
  expect(callKey("b", "b/a/b.jpg")).toBe("a/b.jpg");
  expect(callKey("b", "b")).toBeNull();
  expect(callKey("b", "bb/x")).toBeNull();
  expect(callKey("b", null)).toBeNull();
});

test("a storage class is named as the AWS console names it", () => {
  expect([undefined, "STANDARD_IA", "ONEZONE_IA", "INTELLIGENT_TIERING", "GLACIER_IR", "GLACIER", "DEEP_ARCHIVE"].map(storageWords)).toEqual(["Standard", "Standard-IA", "One Zone-IA", "Intelligent-Tiering", "Glacier Instant Retrieval", "Glacier Flexible Retrieval", "Glacier Deep Archive"]);
  expect(storageWords("SOMETHING_NEW")).toBe("Something new");
});
