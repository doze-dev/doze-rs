import { describe, expect, test } from "bun:test";
import { currentTab } from "./resource-page";

const tabs = [{ id: "details" }, { id: "tags" }];
describe("currentTab", () => {
  test("the tab the address names, when it is one", () => expect(currentTab(tabs, "tags")).toBe("tags"));
  test("any other is the first, never a blank page", () => {
    expect(currentTab(tabs, "gone")).toBe("details");
    expect(currentTab(tabs, undefined)).toBe("details");
    expect(currentTab([], "x")).toBe("");
  });
});
