// The colours of source code in the Code tab's editor: keywords, strings, comments and numbers of Python, JavaScript and Ruby. A
// small tokenizer, not a parser: it never changes the text, only says which stretch is what. Pure.
import type { Family } from "./model";

export type Tok = { kind: "kw" | "str" | "com" | "num" | "plain"; text: string };

const WORDS: Record<Family, string[]> = {
  python: ["False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield"],
  node: ["async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else", "export", "extends", "false", "finally", "for", "from", "function", "if", "import", "in", "instanceof", "let", "new", "null", "of", "return", "switch", "this", "throw", "true", "try", "typeof", "undefined", "var", "while", "yield"],
  ruby: ["BEGIN", "END", "alias", "and", "begin", "break", "case", "class", "def", "defined?", "do", "else", "elsif", "end", "ensure", "false", "for", "if", "in", "module", "next", "nil", "not", "or", "redo", "rescue", "retry", "return", "self", "super", "then", "true", "undef", "unless", "until", "when", "while", "yield", "require"],
};

const PATTERN: Record<Family, RegExp> = {
  python: /(#[^\n]*)|("""[\s\S]*?"""|'''[\s\S]*?'''|"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')|(\b\d[\d_.]*\b)|([A-Za-z_][A-Za-z0-9_]*)/y,
  node: /(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|`(?:\\.|[^`\\])*`)|(\b\d[\d_.]*\b)|([A-Za-z_$][A-Za-z0-9_$]*)/y,
  ruby: /(#[^\n]*)|("(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')|(\b\d[\d_.]*\b)|([A-Za-z_][A-Za-z0-9_]*[?!]?)/y,
};

/** `src` cut into stretches that read as one thing each; joined, they are `src` again. */
export function tokenize(src: string, family: Family): Tok[] {
  const re = new RegExp(PATTERN[family].source, "y");
  const words = new Set(WORDS[family]);
  const out: Tok[] = [];
  let plain = "";
  let at = 0;
  const flush = () => {
    if (plain) out.push({ kind: "plain", text: plain });
    plain = "";
  };
  while (at < src.length) {
    re.lastIndex = at;
    const m = re.exec(src);
    if (!m) {
      plain += src[at];
      at += 1;
      continue;
    }
    const text = m[0];
    const kind: Tok["kind"] = m[1] ? "com" : m[2] ? "str" : m[3] ? "num" : words.has(text) ? "kw" : "plain";
    if (kind === "plain") plain += text;
    else {
      flush();
      out.push({ kind, text });
    }
    at += text.length || 1;
  }
  flush();
  return out;
}
