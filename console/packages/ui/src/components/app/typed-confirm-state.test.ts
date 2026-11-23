import { describe, expect, test } from "bun:test";
import { confirmed } from "./typed-confirm-state";

describe("confirmed", () => {
  test("the name, exactly", () => {
    expect(confirmed("larder-admin", "larder-admin")).toBe(true);
    expect(confirmed("Larder-admin", "larder-admin")).toBe(false);
    expect(confirmed("larder", "larder-admin")).toBe(false);
  });
  test("a trailing space from a paste is forgiven; a leading one is not", () => {
    expect(confirmed("larder-admin ", "larder-admin")).toBe(true);
    expect(confirmed(" larder-admin", "larder-admin")).toBe(false);
  });
  test("nothing asked, nothing confirmed", () => expect(confirmed("", "")).toBe(false));
});
