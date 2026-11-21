import { describe, expect, test } from "bun:test";
import { isStatusColumn, statusTone } from "./status";

describe("the status of an item", () => {
  test("a column named status or state holds one", () => {
    expect(isStatusColumn("status")).toBe(true);
    expect(isStatusColumn("State")).toBe(true);
    expect(isStatusColumn("order_status")).toBe(false);
  });
  test("the usual words have the colour of their meaning, any other none", () => {
    expect(statusTone("delivered")).toBe("ok");
    expect(statusTone("Placed")).toBe("warn");
    expect(statusTone("cancelled")).toBe("bad");
    expect(statusTone("paused")).toBe("idle");
    expect(statusTone("quarantined")).toBeNull();
  });
});
