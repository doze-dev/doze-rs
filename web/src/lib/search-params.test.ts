import { describe, expect, test } from "bun:test";
import { parseSearch, stringifySearch } from "./search-params";

describe("the address's search params", () => {
  test("a list is the same name repeated, not JSON", () => {
    expect(stringifySearch({ topic: ["orders", "clickstream"], tail: true })).toBe("?topic=orders&topic=clickstream&tail=true");
    expect(parseSearch("?topic=orders&topic=clickstream&tail=true")).toEqual({ topic: ["orders", "clickstream"], tail: true });
  });
  test("one value is a value (the page's parser makes it a list of one)", () => {
    expect(stringifySearch({ topic: ["orders"] })).toBe("?topic=orders");
    expect(parseSearch("?topic=orders")).toEqual({ topic: "orders" });
  });
  test("old JSON links keep working", () => {
    expect(parseSearch("?topic=%5B%22orders%22%5D&request=12&clients=true")).toEqual({ topic: ["orders"], request: 12, clients: true });
  });
  test("a text that would read as another type round-trips as text", () => {
    for (const v of ["12", "true", "[x", '"q"']) expect(parseSearch(stringifySearch({ key: v }))).toEqual({ key: v });
    expect(parseSearch(stringifySearch({ partition: 1, time: "2026-11-13 14:30", record: "0:12" }))).toEqual({ partition: 1, time: "2026-11-13 14:30", record: "0:12" });
  });
  test("nothing is no search at all", () => {
    expect(stringifySearch({ a: undefined })).toBe("");
    expect(parseSearch("")).toEqual({});
  });
});
