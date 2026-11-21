import { describe, expect, test } from "bun:test";
import { base64, sendInput } from "./send";

describe("SendMessage", () => {
  const form = { body: "hi", delay: "", group: "", dedup: "", attrs: [] };
  test("a send is its body, its delay or its FIFO ids, and its attributes", () => {
    expect(sendInput({ ...form, delay: "5", attrs: [{ name: "k", type: "Number", value: "2" }] }, false)).toEqual({ MessageBody: "hi", DelaySeconds: 5, MessageAttributes: { k: { DataType: "Number", StringValue: "2" } } });
    expect(sendInput({ ...form, delay: "901" }, false)).toContain("0 to 900");
    expect(sendInput(form, true)).toContain("message group ID");
    expect(sendInput({ ...form, group: "g", dedup: "d" }, true)).toEqual({ MessageBody: "hi", MessageGroupId: "g", MessageDeduplicationId: "d" });
    expect(sendInput({ ...form, body: "" }, false)).toContain("body");
    expect(sendInput({ ...form, attrs: [{ name: "n", type: "Number", value: "x" }] }, false)).toContain("Number");
  });
  test("a FIFO queue's send has no delay, and a binary attribute is base64", () => {
    expect(sendInput({ ...form, delay: "5", group: "g" }, true)).toEqual({ MessageBody: "hi", MessageGroupId: "g" });
    expect(sendInput({ ...form, attrs: [{ name: "b", type: "Binary", value: "hi" }] }, false)).toEqual({ MessageBody: "hi", MessageAttributes: { b: { DataType: "Binary", BinaryValue: "aGk=" } } });
    expect(base64("é")).toBe("w6k=");
  });
});
