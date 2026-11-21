import { useId } from "react";
import { cn } from "../../lib/utils";
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

/** A JSON document typed by a person: monospace, checked as it is typed (the parser's words under it when it is not JSON), and
 * a Format button. The caller decides what an empty document means (`empty`). */
export function JsonEditor({ value, onChange, label, help, placeholder, empty = "ok", rows = 10, className, testId = "json-editor" }: { value: string; onChange: (v: string) => void; label: string; help?: React.ReactNode; /** What an empty editor shows. */ placeholder?: string; empty?: "ok" | "invalid"; rows?: number; className?: string; testId?: string }) {
  const id = useId();
  const check = checkJson(value, empty);
  const bad = !check.ok;
  return (
    <div className={cn("grid gap-1.5", className)}>
      <div className="flex items-end justify-between gap-3">
        <Label htmlFor={id}>{label}</Label>
        <Button type="button" variant="ghost" size="sm" onClick={() => onChange(formatJson(value))} disabled={bad || value.trim() === ""} data-testid={`${testId}-format`}>
          Format
        </Button>
      </div>
      <Textarea
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        rows={rows}
        spellCheck={false}
        autoComplete="off"
        aria-invalid={bad}
        aria-describedby={`${id}-help`}
        className="min-h-32 bg-card font-mono text-[13px] leading-5"
        data-testid={testId}
      />
      <p id={`${id}-help`} className={cn("text-[12.5px] leading-[18px]", bad ? "text-bad" : "text-muted-foreground")} data-testid={`${testId}-help`}>
        {bad ? `Not JSON: ${check.why}` : help}
      </p>
    </div>
  );
}
