// JSON text cut into the pieces a person reads in colour: keys, strings, numbers, true/false/null, and the rest (braces, commas, spaces,
// and the `//` comments some pages write above a document). It never throws: text that is not JSON, or is half typed, is cut as far as
// it can be and the rest is plain, so an editor can colour what is on screen at every keystroke.

export type JsonKind = "key" | "string" | "number" | "literal" | "plain";
export type JsonToken = { text: string; kind: JsonKind };

const NUMBER = /^-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/;
const LITERAL = /^(?:true|false|null)\b/;

/** The text as tokens, which joined give the text back exactly. */
export function tokenizeJson(text: string): JsonToken[] {
  const out: JsonToken[] = [];
  const push = (t: string, kind: JsonKind) => {
    if (t === "") return;
    const last = out[out.length - 1];
    if (last && last.kind === kind && kind === "plain") last.text += t;
    else out.push({ text: t, kind });
  };
  let i = 0;
  while (i < text.length) {
    const c = text[i] ?? "";
    if (c === '"') {
      // A string runs to its closing quote (escapes skipped), or to the end of the line when it is not closed.
      let j = i + 1;
      while (j < text.length && text[j] !== '"' && text[j] !== "\n") j += text[j] === "\\" ? 2 : 1;
      const closed = text[j] === '"';
      const end = closed ? j + 1 : Math.min(j, text.length);
      const rest = text.slice(end);
      // A string followed (past spaces) by a colon is a key.
      push(text.slice(i, end), closed && /^\s*:/.test(rest) ? "key" : "string");
      i = end;
    } else if (c === "/" && text[i + 1] === "/") {
      const nl = text.indexOf("\n", i);
      const end = nl === -1 ? text.length : nl;
      push(text.slice(i, end), "plain");
      i = end;
    } else {
      const rest = text.slice(i);
      const n = c === "-" || (c >= "0" && c <= "9") ? NUMBER.exec(rest) : null;
      const l = LITERAL.exec(rest);
      if (n) {
        push(n[0], "number");
        i += n[0].length;
      } else if (l) {
        push(l[0], "literal");
        i += l[0].length;
      } else {
        push(c, "plain");
        i += 1;
      }
    }
  }
  return out;
}

/** The role colour (a Tailwind class of the theme's own roles) a kind is drawn in; plain text keeps the block's colour. */
export const JSON_COLOUR: Readonly<Record<JsonKind, string>> = {
  key: "text-link",
  string: "text-good",
  number: "text-warn",
  literal: "text-cool",
  plain: "",
};
