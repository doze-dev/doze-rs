import { describe, expect, test } from "bun:test";
import type { KeyPart } from "./json";
import { attributeOf, blankRows, itemOf, keyParam, newAttr, paramKey, rowsOf } from "./item-rows";

const KEYS: KeyPart[] = [{ name: "pk", type: "S", role: "HASH" }, { name: "sk", type: "N", role: "RANGE" }];

describe("an item as rows", () => {
  test("the rows of an item put the keys first, typed, and the rest after", () => {
    const rows = rowsOf({ status: { S: "placed" }, sk: { N: "2" }, pk: { S: "a" }, tags: { SS: ["x", "y"] }, ok: { BOOL: true } }, KEYS);
    expect(rows.map((r) => [r.name, r.type, r.value, r.key])).toEqual([
      ["pk", "S", "a", "HASH"],
      ["sk", "N", "2", "RANGE"],
      ["status", "S", "placed", null],
      ["tags", "SS", '["x","y"]', null],
      ["ok", "BOOL", "true", null],
    ]);
  });
  test("the rows make the item they were made from", () => {
    const item = { pk: { S: "a" }, sk: { N: "2" }, m: { M: { n: { N: "1" } } }, l: { L: [{ S: "x" }] }, b: { B: "AAEC" }, z: { NULL: true as const }, ns: { NS: ["1", "2.5"] } };
    expect(itemOf(rowsOf(item, KEYS))).toEqual({ item });
  });
  test("a new item starts with its keys, empty, and cannot be made until they have values", () => {
    const rows = blankRows(KEYS);
    expect(rows.map((r) => r.key)).toEqual(["HASH", "RANGE"]);
    expect(itemOf(rows)).toEqual({ error: "pk: a key cannot be empty." });
    expect("item" in itemOf([{ ...rows[0]!, value: "a" }, { ...rows[1]!, value: "1" }])).toBe(true);
  });
  test("a blank row is left out, a repeated name and a nameless value are refused", () => {
    const base = [newAttr("pk", "S", "a", "HASH")];
    expect(itemOf([...base, newAttr()])).toEqual({ item: { pk: { S: "a" } } });
    expect(itemOf([...base, newAttr("pk", "S", "b")])).toEqual({ error: "pk is there twice." });
    expect(itemOf([...base, newAttr("", "S", "x")])).toEqual({ error: "Every attribute needs a name." });
  });
  test("each type checks its value in its own way", () => {
    expect(attributeOf(newAttr("n", "N", "x"))).toEqual({ error: "n: a number is needed." });
    expect(attributeOf(newAttr("n", "N", " 24.90 "))).toEqual({ value: { N: "24.90" } });
    expect(attributeOf(newAttr("s", "SS", "a"))).toHaveProperty("error");
    expect(attributeOf(newAttr("s", "NS", '["1","x"]'))).toHaveProperty("error");
    expect(attributeOf(newAttr("l", "L", '[{"S": "a"}]'))).toEqual({ value: { L: [{ S: "a" }] } });
    expect(attributeOf(newAttr("l", "L", '["a"]'))).toHaveProperty("error");
    expect(attributeOf(newAttr("m", "M", "{"))).toHaveProperty("error");
    expect(attributeOf(newAttr("b", "BOOL", "maybe"))).toHaveProperty("error");
  });
});

describe("an item's key in the address", () => {
  test("goes there and back, whatever it holds, and is never JSON the router would read", () => {
    const key = { pk: { S: "ord é/1+2=" }, sk: { N: "2" } };
    const text = keyParam(key);
    expect(text).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(paramKey(text)).toEqual(key);
    expect(paramKey("not a key")).toBeNull();
    expect(paramKey(undefined)).toBeNull();
  });
});
