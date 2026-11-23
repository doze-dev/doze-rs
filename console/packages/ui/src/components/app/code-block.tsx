import { JSON_COLOUR, tokenizeJson } from "../../lib/json-tokens";
import { cn } from "../../lib/utils";

/** JSON text with its keys, strings, numbers and literals in the theme's role colours (the text itself is unchanged: select and copy give it back). */
export function JsonText({ text }: { text: string }) {
  return (
    <>
      {tokenizeJson(text).map((t, i) => (t.kind === "plain" ? t.text : <span key={i} className={JSON_COLOUR[t.kind]} data-json={t.kind}>{t.text}</span>))}
    </>
  );
}

/** A JSON value written out, indented, in a scrolling block. */
export function JsonBlock({ value, className, testId }: { value: unknown; className?: string; testId?: string }) {
  const text = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  return (
    <pre className={cn("max-h-80 overflow-auto rounded-[var(--radius-field)] border bg-muted px-4 py-3 font-mono text-[12.5px] leading-5 whitespace-pre-wrap break-all text-foreground", className)} data-slot="json-block" data-testid={testId}>
      {text}
    </pre>
  );
}

/** A body that is JSON, indented; any other text as it is (none: empty). */
export function pretty(body: string | null | undefined): string {
  if (!body) return "";
  try {
    const v: unknown = JSON.parse(body);
    return typeof v === "object" && v !== null ? JSON.stringify(v, null, 2) : body;
  } catch {
    return body;
  }
}

/** A read-only block of code or text, with its own scroll; wrapped or not. */
export function CodeBlock({ text, wrap = true, className, testId, max = "max-h-[560px]", json = false }: { text: string; wrap?: boolean; className?: string; testId?: string; max?: string; /** The text is JSON: colour its keys, strings, numbers and literals. */ json?: boolean }) {
  return (
    <pre tabIndex={0} className={cn("overflow-auto rounded-[var(--radius-panel)] border bg-well px-5 py-4 font-mono text-[12.5px] leading-[21px] text-foreground outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50", max, wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre", className)} data-slot="code-block" data-testid={testId}>
      {json ? <JsonText text={text} /> : text}
    </pre>
  );
}
