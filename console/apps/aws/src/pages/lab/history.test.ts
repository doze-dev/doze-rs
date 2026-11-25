import { describe, expect, test } from "bun:test";
import type { LabAnswer } from "../../api/types/LabAnswer";
import { addSent, type Ask, MOST_INPUT, MOST_SENT, parseSent, sentFrom, summarise } from "./history";
import { keyed, parseList } from "./kept";

const ask: Ask = { target: "sqs", service: "sqs", title: "SQS", op: "SendMessage", input: { QueueUrl: "http://127.0.0.1:4566/111122223333/orders", MessageBody: "hello" } };
const ok: LabAnswer = { output: "{}", error: null, seq: 7, took_us: 1200 };
const bad: LabAnswer = { output: null, error: { status: 400, code: "AWS.SimpleQueueService.NonExistentQueue", message: "no such queue" }, seq: 8, took_us: 300 };

describe("the history of calls sent", () => {
  test("a line says which thing, not where it lives", () => {
    expect(summarise(ask.input)).toBe('QueueUrl: "orders", MessageBody: "hello"');
    expect(summarise({})).toBe("No input");
    expect(summarise({ Entries: [1, 2], Item: { a: 1 }, N: 3, B: true })).toBe("Entries: [2], Item: {…}, N: 3, B: true");
    expect(summarise({ MessageBody: "x".repeat(60) }, 30).length).toBe(30);
  });

  test("an entry holds the answer's outcome and what is needed to send it again", () => {
    const s = sentFrom(ask, ok, "a", 1000);
    expect(s).toMatchObject({ id: "a", at: 1000, op: "SendMessage", ok: true, code: null, took_us: 1200, input: ask.input });
    const f = sentFrom(ask, bad, "b", 2000);
    expect(f).toMatchObject({ ok: false, code: "AWS.SimpleQueueService.NonExistentQueue", status: 400 });
  });

  test("an input too long is not kept, and the entry says so by holding none", () => {
    const big: Ask = { ...ask, input: { MessageBody: "x".repeat(MOST_INPUT + 1) } };
    expect(sentFrom(big, ok, "c", 1).input).toBeNull();
  });

  test("the list keeps the last fifty, newest first", () => {
    let list = [] as ReturnType<typeof sentFrom>[];
    for (let i = 0; i < MOST_SENT + 5; i++) list = addSent(list, sentFrom(ask, ok, `id${i}`, i));
    expect(list).toHaveLength(MOST_SENT);
    expect(list[0]?.id).toBe(`id${MOST_SENT + 4}`);
    expect(list[MOST_SENT - 1]?.id).toBe("id5");
  });

  test("what was kept is read back, and what is not whole is dropped", () => {
    const s = sentFrom(ask, ok, "a", 1000);
    const text = JSON.stringify([s, { id: "x" }, 4, null, { ...s, id: "kept-no-input", input: null }, { ...s, op: "" }]);
    const back = parseList(text, parseSent);
    expect(back.map((e) => e.id)).toEqual(["a", "kept-no-input"]);
    expect(back[0]).toEqual(s);
    expect(parseList("not json", parseSent)).toEqual([]);
    expect(parseList(null, parseSent)).toEqual([]);
    expect(parseList('{"a":1}', parseSent)).toEqual([]);
  });

  test("each region keeps a list of its own", () => {
    expect(keyed("doze-aws.lab.sent", null)).toBe("doze-aws.lab.sent.default");
    expect(keyed("doze-aws.lab.sent", "eu-west-1")).toBe("doze-aws.lab.sent.eu-west-1");
  });
});
