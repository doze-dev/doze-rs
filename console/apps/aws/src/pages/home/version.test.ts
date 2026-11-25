import { expect, test } from "bun:test";
import { shownVersion } from "./version";

test("a release number is shown; the placeholder of a build with none, and nothing, are not", () => {
  expect(shownVersion("0.2.1")).toBe("0.2.1");
  expect(shownVersion("1.0.0-rc.1")).toBe("1.0.0-rc.1");
  expect(shownVersion("0.0.0")).toBeNull();
  expect(shownVersion("0.0.0-dev+abc123")).toBeNull();
  expect(shownVersion("")).toBeNull();
  expect(shownVersion(undefined)).toBeNull();
  // Only the placeholder: a real 0.0.x or 0.x release is a version.
  expect(shownVersion("0.0.1")).toBe("0.0.1");
  expect(shownVersion("0.10.0")).toBe("0.10.0");
});
