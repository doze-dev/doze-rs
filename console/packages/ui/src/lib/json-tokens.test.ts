import { describe, expect, test } from "bun:test";
import { tokenizeJson } from "./json-tokens";

const kinds = (text: string) => tokenizeJson(text).filter((t) => t.kind !== "plain").map((t) => `${t.kind}:${t.text}`);

describe("JSON cut for colour", () => {
  test("keys, strings, numbers and literals are told apart", () => {
    expect(kinds('{"Effect": "Allow", "n": -1.5e3, "ok": true, "x": null}')).toEqual(['key:"Effect"', 'string:"Allow"', 'key:"n"', "number:-1.5e3", 'key:"ok"', "literal:true", 'key:"x"', "literal:null"]);
  });
  test("a string with an escaped quote is one string, and a colon inside it does not make it a key", () => {
    expect(kinds('["a\\"b: c", "d"]')).toEqual(['string:"a\\"b: c"', 'string:"d"']);
  });
  test("the pieces joined are the text, whatever the text is", () => {
    for (const t of ['{"a":[1,2,{"b":"c"}]}', "{a:1", '{"open', "", "// note\n{}", '"x" : 1', "tru", "- 5 -", '"a\\']) expect(tokenizeJson(t).map((p) => p.text).join("")).toBe(t);
  });
  test("an unclosed string is a string to the end of its line, and the next line goes on", () => {
    expect(kinds('{"a": "open\n"b": 1}')).toEqual(['key:"a"', 'string:"open', 'key:"b"', "number:1"]);
  });
  test("a comment line is plain, and what it says is not coloured", () => {
    expect(kinds('// the "role" is 5 times true\n{"a": 1}')).toEqual(['key:"a"', "number:1"]);
  });
  test("a word that only starts like a literal is plain", () => {
    expect(kinds("[nullable, trueish]")).toEqual([]);
  });
});
