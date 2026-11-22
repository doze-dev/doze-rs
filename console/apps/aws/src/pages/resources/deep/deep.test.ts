import { describe, expect, test } from "bun:test";
import { cell, exactInJson, fromPlain, type Item, itemPlainBlocker, itemText, keyOf, type KeyPart, parseItem, toPlain } from "./ddb-json";
import { crumbsOf, rowsOf } from "./s3-objects";
import { folderWrong } from "./s3-dialogs";
import { sendInput } from "./sqs-send";
import { changed } from "./sqs-settings";

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

describe("S3 objects", () => {
  test("a listing is its folders, then its objects, without the folder's own marker", () => {
    const rows = rowsOf({ CommonPrefixes: [{ Prefix: "a/b/" }], Contents: [{ Key: "a/" }, { Key: "a/x.txt", Size: 3 }] }, "a/");
    expect(rows.map((r) => [r.key, r.folder])).toEqual([
      ["a/b/", true],
      ["a/x.txt", false],
    ]);
  });

  test("a prefix is its crumbs, and a folder's name holds no slash", () => {
    expect(crumbsOf("a/b/")).toEqual([
      { label: "a", prefix: "a/" },
      { label: "b", prefix: "a/b/" },
    ]);
    expect(crumbsOf("")).toEqual([]);
    expect(folderWrong("a/b")).toContain("cannot hold");
    expect(folderWrong("banners")).toBeNull();
  });
});

describe("SQS", () => {
  const form = { body: "hi", delay: "", group: "", dedup: "", attrs: [] };
  test("a send is its body, its delay or its FIFO ids, and its attributes", () => {
    expect(sendInput({ ...form, delay: "5", attrs: [{ name: "k", type: "Number", value: "2" }] }, false)).toEqual({ MessageBody: "hi", DelaySeconds: 5, MessageAttributes: { k: { DataType: "Number", StringValue: "2" } } });
    expect(sendInput({ ...form, delay: "901" }, false)).toContain("0 to 900");
    expect(sendInput(form, true)).toContain("message group ID");
    expect(sendInput({ ...form, group: "g", dedup: "d" }, true)).toEqual({ MessageBody: "hi", MessageGroupId: "g", MessageDeduplicationId: "d" });
    expect(sendInput({ ...form, body: "" }, false)).toContain("body");
    expect(sendInput({ ...form, attrs: [{ name: "n", type: "Number", value: "x" }] }, false)).toContain("Number");
  });

  test("settings send only what changed, a dead-letter queue as its ARN", () => {
    const was = { VisibilityTimeout: "30", MessageRetentionPeriod: "345600", DelaySeconds: "0", ReceiveMessageWaitTimeSeconds: "0", MaximumMessageSize: "262144", dlq: "", maxReceives: "", dedup: "false" };
    const arn = (q: string) => `arn:aws:sqs:us-east-1:000000000000:${q}`;
    expect(changed(was, was, arn, false)).toEqual({});
    expect(changed(was, { ...was, VisibilityTimeout: "45" }, arn, false)).toEqual({ VisibilityTimeout: "45" });
    expect(changed(was, { ...was, VisibilityTimeout: "50000" }, arn, false)).toContain("43,200");
    expect(changed(was, { ...was, dlq: "dlq", maxReceives: "3" }, arn, false)).toEqual({ RedrivePolicy: JSON.stringify({ deadLetterTargetArn: arn("dlq"), maxReceiveCount: 3 }) });
    expect(changed({ ...was, dlq: "dlq", maxReceives: "3" }, was, arn, false)).toEqual({ RedrivePolicy: "" });
    expect(changed(was, { ...was, dedup: "true" }, arn, true)).toEqual({ ContentBasedDeduplication: "true" });
  });
});
