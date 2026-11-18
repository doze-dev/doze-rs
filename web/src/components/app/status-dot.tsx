import { cn } from "@/lib/utils";

export type Tone = "ok" | "warn" | "bad" | "idle" | "info";

const tones: Record<Tone, string> = {
  ok: "text-good",
  warn: "text-warn",
  bad: "text-bad",
  idle: "text-muted-foreground",
  info: "text-cool",
};

/** A state word with a dot of its colour (`Running`, `Reachable`, `Not checked`). The word carries the meaning; the colour
 * only repeats it. */
export function StatusDot({ tone, children, className, ...rest }: { tone: Tone; children: React.ReactNode; className?: string } & React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span {...rest} className={cn("inline-flex items-center gap-1.5 text-[13px] font-semibold whitespace-nowrap", tones[tone], className)} data-tone={tone}>
      <span aria-hidden className="size-2 shrink-0 rounded-full bg-current" />
      {children}
    </span>
  );
}
