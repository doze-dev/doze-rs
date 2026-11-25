import { useId, useRef } from "react";
import { cn } from "../../lib/utils";
import { JsonText } from "./code-block";
import { Button } from "../ui/button";
import { Label } from "../ui/label";
import { Textarea } from "../ui/textarea";

/** Whether `text` is JSON, and where it is not: the parser's own words. Empty text is `empty`. */
export function checkJson(text: string, empty: "ok" | "invalid" = "ok"): { ok: true; value: unknown } | { ok: false; why: string } {
  if (text.trim() === "") return empty === "ok" ? { ok: true, value: undefined } : { ok: false, why: "It is empty." };
  try {
    return { ok: true, value: JSON.parse(text) as unknown };
  } catch (e) {
    return { ok: false, why: (e as Error).message };
  }
}

/** `text` indented two spaces when it is JSON; as it is otherwise. */
export function formatJson(text: string): string {
  const c = checkJson(text);
  return c.ok && c.value !== undefined ? JSON.stringify(c.value, null, 2) : text;
}

/** A JSON document typed by a person: monospace, checked as it is typed (the parser's words under it when it is not JSON). The caller
 * decides what an empty document means (`empty`). A Format button is there only when asked for (`format`): no board draws one beside a JSON
 * input, so an editor has none unless its page has a reason to. */
export function JsonEditor({ value, onChange, label, help, placeholder, empty = "ok", rows = 10, className, testId = "json-editor", highlight = false, format = false }: { value: string; onChange: (v: string) => void; label: string; help?: React.ReactNode; /** What an empty editor shows. */ placeholder?: string; empty?: "ok" | "invalid"; rows?: number; className?: string; testId?: string; /** Colour the keys, strings, numbers and literals as they are typed (the text is drawn under a transparent field). */ highlight?: boolean; /** A Format button beside the label, which indents what is typed. */ format?: boolean }) {
  const id = useId();
  const under = useRef<HTMLPreElement>(null);
  const check = checkJson(value, empty);
  const bad = !check.ok;
  return (
    <div className={cn("grid gap-1.5 [&>*]:min-w-0", className)}>
      <div className="flex items-end justify-between gap-3">
        <Label htmlFor={id}>{label}</Label>
        {format && (
          <Button type="button" variant="ghost" size="sm" onClick={() => onChange(formatJson(value))} disabled={bad || value.trim() === ""} data-testid={`${testId}-format`}>
            Format
          </Button>
        )}
      </div>
      <div className="relative">
        {highlight && (
          <pre ref={under} aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden rounded-md border border-transparent bg-card px-3 py-2 font-mono text-[13px] leading-5 break-words whitespace-pre-wrap text-foreground md:text-sm" data-testid={`${testId}-colours`}>
            <JsonText text={value} />
            {"\n"}
          </pre>
        )}
        <Textarea
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onScroll={highlight ? (e) => { if (under.current) under.current.scrollTop = e.currentTarget.scrollTop; } : undefined}
          placeholder={placeholder}
          rows={rows}
          spellCheck={false}
          autoComplete="off"
          aria-invalid={bad}
          aria-describedby={`${id}-help`}
          className={cn("relative min-h-32 w-full max-w-full bg-card font-mono text-[13px] leading-5", highlight && "bg-transparent text-transparent caret-foreground dark:bg-transparent")}
          data-testid={testId}
        />
      </div>
      <p id={`${id}-help`} className={cn("text-[12.5px] leading-[18px]", bad ? "text-bad" : "text-muted-foreground")} data-testid={`${testId}-help`}>
        {bad ? `Not JSON: ${check.why}` : help}
      </p>
    </div>
  );
}
