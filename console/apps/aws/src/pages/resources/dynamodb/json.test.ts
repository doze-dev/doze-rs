import { describe, expect, test } from "bun:test";
import { cell, exactInJson, fromPlain, type Item, itemPlainBlocker, itemText, keyOf, type KeyPart, parseItem, toPlain } from "./json";

const KEYS: KeyPart[] = [
  { name: "pk", type: "S", role: "HASH" },
  { name: "sk", type: "N", role: "RANGE" },
];

describe("DynamoDB JSON", () => {
  test("plain JSON and DynamoDB JSON go both ways", () => {
    const plain = { pk: "a", sk: 2, tags: ["x", 1], nested: { ok: true, none: null } };
    const typed = Object.fromEntries(Object.entries(plain).map(([k, v]) => [k, fromPlain(v)]));
    expect(typed.sk).toEqual({ N: "2" });
    expect(typed.tags).toEqual({ L: [{ S: "x" }, { N: "1" }] });
    expect(typed.nested).toEqual({ M: { ok: { BOOL: true }, none: { NULL: true } } });
    expect(Object.fromEntries(Object.entries(typed).map(([k, v]) => [k, toPlain(v)]))).toEqual(plain);
  });

  test("an item with a set, a binary or a too precise number is not plain", () => {
    expect(itemPlainBlocker({ pk: { S: "a" }, s: { SS: ["x"] } })).toBe("s: it holds a set");
    expect(itemPlainBlocker({ b: { B: "AAEC" } })).toBe("b: it holds a binary value");
    expect(itemPlainBlocker({ n: { N: "12345678901234567890" } })).toContain("more precise");
    expect(itemPlainBlocker({ n: { N: "19.250" }, m: { M: { x: { N: "-0.5e3" } } } })).toBeNull();
    expect(exactInJson("1e400")).toBe(false);
  });

  test("an item is checked against the table's keys, in either mode", () => {
    expect(parseItem('{"pk": "a", "sk": 1}', "plain", KEYS)).toEqual({ item: { pk: { S: "a" }, sk: { N: "1" } } });
    expect(parseItem('{"pk": {"S": "a"}}', "dynamodb", KEYS)).toEqual({ error: "Every item needs its sort key, sk." });
    expect(parseItem('{"pk": {"S": "a"}, "sk": {"S": "1"}}', "dynamodb", KEYS)).toEqual({ error: "sk is a key of type N; this gives it S." });
    expect(parseItem('{"pk": "a", "sk": 1}', "dynamodb", KEYS)).toHaveProperty("error");
    expect(parseItem('{"pk": {"S": "a"}, "sk": {"N": "x"}}', "dynamodb", KEYS)).toHaveProperty("error");
    expect(parseItem('{"pk": "", "sk": 1}', "plain", KEYS)).toEqual({ error: "pk is a key, and a key cannot be empty: give it a value." });
    expect(parseItem('{"pk": {"S": ""}, "sk": {"N": "1"}}', "dynamodb", KEYS)).toHaveProperty("error");
    expect(parseItem('{"pk": "a", "sk": 1, "note": ""}', "plain", KEYS)).toEqual({ item: { pk: { S: "a" }, sk: { N: "1" }, note: { S: "" } } });
    expect(parseItem("[1]", "plain", KEYS)).toEqual({ error: "An item is a JSON object of its attributes." });
    expect("error" in parseItem("{", "plain", KEYS)).toBe(true);
  });

  test("a cell, an item's key and its text", () => {
    const item: Item = { pk: { S: "a" }, sk: { N: "2" }, l: { L: [{ S: "x" }] } };
    expect([cell(item.pk), cell(item.sk), cell(item.l), cell(undefined)]).toEqual(["a", "2", '["x"]', ""]);
    expect(keyOf(item, KEYS)).toEqual({ pk: { S: "a" }, sk: { N: "2" } });
    expect(JSON.parse(itemText(item, "plain"))).toEqual({ pk: "a", sk: 2, l: ["x"] });
  });
});
