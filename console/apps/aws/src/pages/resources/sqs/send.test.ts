import { describe, expect, test } from "bun:test";
import { base64, delaySeconds, sendInput, type SendForm } from "./send";

describe("SendMessage", () => {
  const form: SendForm = { body: "hi", delay: "", delayUnit: "seconds", group: "", dedup: "", attrs: [] };
  const attr = (a: Partial<SendForm["attrs"][number]>) => ({ name: "k", type: "String" as const, custom: "", value: "v", ...a });
  test("a send is its body, its delay or its FIFO ids, and its attributes", () => {
    expect(sendInput({ ...form, delay: "5", attrs: [attr({ type: "Number", value: "2" })] }, false)).toEqual({ MessageBody: "hi", DelaySeconds: 5, MessageAttributes: { k: { DataType: "Number", StringValue: "2" } } });
    expect(sendInput({ ...form, delay: "901" }, false)).toContain("0 to 900");
    expect(sendInput(form, true)).toContain("message group ID");
    expect(sendInput({ ...form, group: "g", dedup: "d" }, true)).toEqual({ MessageBody: "hi", MessageGroupId: "g", MessageDeduplicationId: "d" });
    expect(sendInput({ ...form, body: "" }, false)).toContain("body");
    expect(sendInput({ ...form, attrs: [attr({ name: "n", type: "Number", value: "x" })] }, false)).toContain("Number");
  });
  test("a FIFO queue's send has no delay, and a binary attribute is base64", () => {
    expect(sendInput({ ...form, delay: "5", group: "g" }, true)).toEqual({ MessageBody: "hi", MessageGroupId: "g" });
    expect(sendInput({ ...form, attrs: [attr({ name: "b", type: "Binary", value: "hi" })] }, false)).toEqual({ MessageBody: "hi", MessageAttributes: { b: { DataType: "Binary", BinaryValue: "aGk=" } } });
    expect(base64("é")).toBe("w6k=");
  });
  test("the delay is read in its unit, up to 15 minutes", () => {
    expect(delaySeconds("2", "minutes")).toBe(120);
    expect(delaySeconds("15", "minutes")).toBe(900);
    expect(delaySeconds("16", "minutes")).toContain("15 minutes");
    expect(delaySeconds("1.5", "seconds")).toContain("whole");
    expect(sendInput({ ...form, delay: "1", delayUnit: "minutes" }, false)).toEqual({ MessageBody: "hi", DelaySeconds: 60 });
  });
  test("a custom type is the type's suffix", () => {
    expect(sendInput({ ...form, attrs: [attr({ custom: "label" })] }, false)).toEqual({ MessageBody: "hi", MessageAttributes: { k: { DataType: "String.label", StringValue: "v" } } });
  });
});
