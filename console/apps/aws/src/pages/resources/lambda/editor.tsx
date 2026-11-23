// A text file edited in the page: a textarea over its own coloured copy, so the text is the browser's own (caret, selection, undo,
// the keyboard) and only the colours are ours. The copy and the textarea share their font and padding, line for line.
import { useMemo } from "react";
import { cn } from "@doze/ui/lib/utils";
import { tokenize } from "./highlight";
import type { Family } from "./model";

const TONE: Record<string, string> = { kw: "text-primary", str: "text-good", com: "text-muted-foreground italic", num: "text-cool", plain: "" };
const SHARED = "m-0 min-w-full w-max whitespace-pre px-5 py-4 font-mono text-[13px] leading-[21px] tracking-normal [tab-size:4]";

export function CodeEditor({ value, onChange, family, label, readOnly = false, testId = "code-editor" }: { value: string; onChange: (v: string) => void; family: Family; label: string; readOnly?: boolean; testId?: string }) {
  const tokens = useMemo(() => tokenize(value, family), [value, family]);
  const lines = value.split("\n").length;
  return (
    <div className="relative max-h-[28rem] overflow-auto rounded-[12px] border bg-well focus-within:ring-[3px] focus-within:ring-ring/50" data-testid={`${testId}-box`}>
      <div className="relative">
        <pre aria-hidden className={cn(SHARED, "pointer-events-none text-foreground")} data-testid={`${testId}-colours`}>
          {tokens.map((t, i) => (
            <span key={i} className={TONE[t.kind]}>{t.text}</span>
          ))}
          {"\n"}
        </pre>
        <textarea
          aria-label={label}
          value={value}
          readOnly={readOnly}
          onChange={(e) => onChange(e.target.value)}
          rows={lines + 1}
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
