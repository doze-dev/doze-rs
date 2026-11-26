import { expect, test } from "bun:test";
import { doneOf, unchangedKey } from "./done-words";

test("a publish that answered a version the list already had says no version was made", () => {
  expect(doneOf("published", "send-email")?.title).toBe("Version published.");
  const same = doneOf(unchangedKey("3"), "send-email");
  expect(same?.title).toBe("No new version was made.");
  expect(same?.returned).toContain("version 3");
});

test("what a row delete leaves is said as a banner, as a layer version's is", () => {
  expect(["trigger-deleted", "permission-deleted", "url-deleted", "alias-deleted"].map((k) => doneOf(k, "f")?.title)).toEqual(["Trigger deleted.", "Permission deleted.", "Function URL deleted.", "Alias deleted."]);
  expect(doneOf("nothing-like-it", "f")).toBeUndefined();
});
