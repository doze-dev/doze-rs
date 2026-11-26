import { describe, expect, test } from "bun:test";
import { bytesOf, splitBody, withBody } from "./body-box";

describe("the body of a put", () => {
  test("the size is the bytes sent, not the characters typed", () => {
    expect(bytesOf("abc")).toBe(3);
    expect(bytesOf("héllo")).toBe(6);
    expect(bytesOf("")).toBe(0);
  });

  test("a body goes into the input as its Body member, and an empty box leaves the editor's input alone", () => {
    expect(withBody({ Bucket: "b", Key: "k" }, "hi")).toEqual({ Bucket: "b", Key: "k", Body: "hi" });
    expect(withBody({ Bucket: "b", Body: "from the editor" }, "")).toEqual({ Bucket: "b", Body: "from the editor" });
    expect(withBody({ Bucket: "b", Body: "from the editor" }, "box")).toEqual({ Bucket: "b", Body: "box" });
  });

  test("an input with a text Body is taken apart so the box holds the body and the editor the rest", () => {
    expect(splitBody({ Bucket: "b", Body: "hi" })).toEqual({ input: { Bucket: "b" }, body: "hi" });
    expect(splitBody({ Bucket: "b" })).toEqual({ input: { Bucket: "b" }, body: "" });
    expect(splitBody({ Bucket: "b", Body: 3 })).toEqual({ input: { Bucket: "b", Body: 3 }, body: "" });
  });
});
