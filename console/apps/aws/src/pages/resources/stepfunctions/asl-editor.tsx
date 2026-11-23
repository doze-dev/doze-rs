// The Amazon States Language text of Workflow Studio's Code mode: a textarea over its own coloured copy, so the text is the browser's
// own (caret, selection, undo) and only the colours are ours. `marked` lines (where the definition is wrong) are drawn in the link
// colour, as the board draws the line that names a state that is not there.
import { useMemo } from "react";
import { cn } from "@doze/ui/lib/utils";

export type Tok = { kind: "key" | "str" | "num" | "word" | "plain"; text: string };

/** The tokens of a JSON text, every character kept (so the coloured copy lines up with the textarea). Tolerant of text that is not JSON. */
export function tokenizeJson(text: string): Tok[] {
  const out: Tok[] = [];
  const re = /("(?:[^"\\\n]|\\.)*"?)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b|([\s\S])/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    if (m[1] !== undefined) {
      const after = /^\s*:/.test(text.slice(re.lastIndex, re.lastIndex + 40));
      out.push({ kind: after ? "key" : "str", text: m[1] });
    } else if (m[2] !== undefined) out.push({ kind: "num", text: m[2] });
    else if (m[3] !== undefined) out.push({ kind: "word", text: m[3] });
    else out.push({ kind: "plain", text: m[4] ?? "" });
  }
  return out;
}

const TONE: Record<Tok["kind"], string> = { key: "", str: "text-good", num: "text-cool", word: "text-primary", plain: "" };
const SHARED = "m-0 min-w-full w-max whitespace-pre px-6 py-5 font-mono text-[13px] leading-[22px] tracking-normal [tab-size:2]";

/** Splits tokens into lines, each a list of tokens (a token that spans a newline cannot: strings stop at one). */
function lines(tokens: Tok[]): Tok[][] {
  const rows: Tok[][] = [[]];
  for (const t of tokens) {
    if (t.text === "\n") rows.push([]);
    else rows[rows.length - 1]?.push(t);
  }
  return rows;
}

export function AslEditor({ value, onChange, marked = [], label = "Amazon States Language", readOnly = false, testId = "asl-editor" }: { value: string; onChange: (v: string) => void; marked?: number[]; label?: string; readOnly?: boolean; testId?: string }) {
  const rows = useMemo(() => lines(tokenizeJson(value)), [value]);
  return (
    <div className="relative overflow-auto rounded-[12px] border bg-well focus-within:ring-[3px] focus-within:ring-ring/50" data-testid={`${testId}-box`}>
      <div className="relative">
        <pre aria-hidden className={cn(SHARED, "pointer-events-none text-foreground")} data-testid={`${testId}-colours`}>
          {rows.map((row, i) => (
            <span key={i} className={cn("block", marked.includes(i + 1) && "text-link")}>
              {row.length === 0 ? "​" : row.map((t, j) => <span key={j} className={marked.includes(i + 1) ? undefined : TONE[t.kind]}>{t.text}</span>)}
            </span>
          ))}
        </pre>
        <textarea
          aria-label={label}
          value={value}
          readOnly={readOnly}
          onChange={(e) => onChange(e.target.value)}
          rows={rows.length}
          wrap="off"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          className={cn(SHARED, "absolute inset-0 h-full resize-none overflow-hidden border-0 bg-transparent text-transparent caret-foreground outline-none selection:bg-primary/25")}
          data-testid={testId}
        />
      </div>
    </div>
  );
}
