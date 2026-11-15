// What the Schemas pages share: a schema type's badge, a level with where it comes from, a schema laid out with Copy, a time.
import { Badge } from "@/components/ui/badge";
import { CopyButton } from "@/components/app/copy-button";
import { cn } from "@/lib/utils";
import { KIND_WORD } from "./search";

export function KindBadge({ kind, className }: { kind: string; className?: string }) {
  return (
    <Badge variant="outline" className={cn("rounded-full bg-card font-mono text-[11.5px] font-normal text-muted-foreground", className)} title={KIND_WORD[kind] ?? kind}>
      {kind}
    </Badge>
  );
}

/** `BACKWARD`, or `BACKWARD (global)` when the subject has none of its own. */
export function LevelWord({ level, own }: { level: string; own: boolean }) {
  return (
    <span className="truncate font-mono text-[12.5px]">
      {level}
      {!own && <span className="ml-1.5 font-sans text-[12px] text-muted-foreground">(global)</span>}
    </span>
  );
}

/** `2026-11-14 09:12 UTC` from RFC 3339. */
export function utcDay(rfc: string): string {
  const m = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})/.exec(rfc);
  return m ? `${m[1]} ${m[2]} UTC` : rfc;
}

/** A schema, laid out, with Copy: scrolls inside itself, never sideways on the page. */
export function SchemaBlock({ text, testId, label = "Copy schema", className }: { text: string; testId?: string; label?: string; className?: string }) {
  return (
    <div className={cn("relative", className)}>
      <pre data-testid={testId} className="max-h-[32rem] overflow-auto rounded-lg border bg-well px-4 py-3 pr-12 font-mono text-[12.5px] leading-5 whitespace-pre text-bright">
        {text}
      </pre>
      <div className="absolute top-2 right-2">
        <CopyButton text={text} label={label} what="Schema copied" iconOnly variant="ghost" />
      </div>
    </div>
  );
}
