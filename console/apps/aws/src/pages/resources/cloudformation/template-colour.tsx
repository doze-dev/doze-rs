// A template as text with its parts coloured (cloudformation__stack-template): keys, strings, numbers and booleans, the intrinsic functions
// (`!Ref`, `!Sub`, `Fn::GetAtt`) and comments, for a template in YAML or in JSON. Each part is a role (a class of the theme), never a colour.
import { cn } from "@doze/ui/lib/utils";

export type Kind = "key" | "string" | "number" | "word" | "tag" | "comment" | "plain";
export type Token = { text: string; kind: Kind };

const CLASS: Record<Kind, string | undefined> = { key: "text-link", string: "text-good", number: "text-cool", word: "text-cool", tag: "text-warn", comment: "text-muted-foreground italic", plain: undefined };

// A value's parts, left to right: a comment, a quoted string, an intrinsic function, a number, a word that is a value.
const VALUE = /(#.*$)|("(?:[^"\\]|\\.)*"|'(?:[^']|'')*')|(!(?:[A-Za-z]+)|\bFn::[A-Za-z]+|\bRef\b)|(?<![\w.-])(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)(?![\w.-])|(?<![\w.-])(true|false|null|True|False|Null)(?![\w.-])/g;

function values(text: string, out: Token[], jsonKeys: boolean): void {
  let at = 0;
  for (const m of text.matchAll(VALUE)) {
    const start = m.index;
    // A `#` inside a word is not a comment (`#` opens one only at the start or after a space).
    if (m[1] !== undefined && start > 0 && !/\s/.test(text[start - 1] ?? "")) continue;
    if (start > at) out.push({ text: text.slice(at, start), kind: "plain" });
    const kind: Kind = m[1] !== undefined ? "comment" : m[2] !== undefined ? (jsonKeys && /^\s*:/.test(text.slice(start + m[0].length)) ? "key" : "string") : m[3] !== undefined ? "tag" : m[4] !== undefined ? "number" : "word";
    out.push({ text: m[0], kind });
    at = start + m[0].length;
  }
  if (at < text.length) out.push({ text: text.slice(at), kind: "plain" });
}

/** One line of a template as its parts. YAML: the key before the first `: ` is a key; JSON: a string followed by `:` is. */
export function colourLine(line: string, json: boolean): Token[] {
  const out: Token[] = [];
  if (json) {
    values(line, out, true);
    return out;
  }
  const key = /^(\s*(?:-\s+)*)(?!#)((?:"[^"]*"|'[^']*'|(?:[^\s:#'"]|::)(?:[^:#]|::)*?))(:)(?=\s|$)/.exec(line);
  if (key) {
    if (key[1]) out.push({ text: key[1], kind: "plain" });
    out.push({ text: key[2]!, kind: "key" }, { text: key[3]!, kind: "plain" });
    values(line.slice(key[0].length), out, false);
  } else values(line, out, false);
  return out;
}

/** A template, coloured, with its own scroll: the CodeBlock's look. `wrap`: long lines wrap (else they scroll). */
export function TemplateCode({ text, json, wrap, testId, max = "max-h-[1600px]" }: { text: string; json: boolean; wrap: boolean; testId?: string; max?: string }) {
  const lines = text.split("\n");
  return (
    <pre tabIndex={0} className={cn("overflow-auto rounded-[12px] border bg-well px-5 py-4 font-mono text-[12.5px] leading-[21px] text-foreground outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50", max, wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre")} data-slot="code-block" data-testid={testId}>
      {lines.map((line, i) => (
        <span key={i}>
          {colourLine(line, json).map((t, j) => (CLASS[t.kind] ? <span key={j} className={CLASS[t.kind]} data-kind={t.kind}>{t.text}</span> : t.text))}
          {i < lines.length - 1 ? "\n" : ""}
        </span>
      ))}
    </pre>
  );
}
