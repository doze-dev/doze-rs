import { expect, test } from "bun:test";
import { SseParser } from "./sse";

test("events complete on a blank line, across chunks and line endings", () => {
  const p = new SseParser();
  expect(p.push("event: call\r\ndata: one\r")).toEqual([]);
  expect(p.push("\ndata: two\n\n")).toEqual([{ event: "call", data: "one\ntwo", id: undefined }]);
  expect(p.push("data: x\n\ndata: y\n\n")).toEqual([
    { event: "message", data: "x", id: undefined },
    { event: "message", data: "y", id: undefined },
  ]);
});

test("comments are skipped, an id is kept for its event only, and one with a NUL is ignored", () => {
  const p = new SseParser();
  expect(p.push(": keep-alive\n\nid: 41\ndata: a\n\ndata: b\n\nid: 4\u00002\ndata: c\n\n")).toEqual([
    { event: "message", data: "a", id: "41" },
    { event: "message", data: "b", id: undefined },
    { event: "message", data: "c", id: undefined },
  ]);
});

test("a field without a colon has an empty value, and only one leading space is taken", () => {
  const p = new SseParser();
  expect(p.push("data\ndata:  two spaces\n\n")).toEqual([{ event: "message", data: "\n two spaces", id: undefined }]);
});

test("a CRLF cut in two by the network is one line ending", () => {
  const p = new SseParser();
  expect(p.push("data: a\r")).toEqual([]);
  expect(p.push("\ndata: b\r\n\r")).toEqual([]);
  expect(p.push("\n")).toEqual([{ event: "message", data: "a\nb", id: undefined }]);
});

test("an event with no data is not an event", () => {
  expect(new SseParser().push("event: ping\n\n")).toEqual([]);
});
