import { CircleAlertIcon, CircleCheckIcon, InfoIcon, type LucideIcon, TriangleAlertIcon } from "lucide-react";
import { cn } from "../../lib/utils";

export type CalloutTone = "info" | "warn" | "good" | "bad" | "note";

const TONES: Record<CalloutTone, { box: string; ink: string; icon: LucideIcon }> = {
  info: { box: "border-cool/30 bg-cool/10", ink: "text-cool", icon: InfoIcon },
  warn: { box: "border-warn/30 bg-warn/10", ink: "text-warn", icon: TriangleAlertIcon },
  good: { box: "border-good/30 bg-good/10", ink: "text-good", icon: CircleCheckIcon },
  bad: { box: "border-bad/30 bg-bad/10", ink: "text-bad", icon: CircleAlertIcon },
  note: { box: "border-border bg-well", ink: "text-muted-foreground", icon: InfoIcon },
};

/** A paragraph that matters, in a box with an icon: a bold lead (`Before you start.`) and the sentence that follows it, on one
 * line of text. Use it where a note must not be missed (a warning before a write, what a permission needs); `RuleHelp` is for the
 * rule of one field. `bad` is announced at once (`role="alert"`), the others politely (`role="status"`: a note that is there from the start is not news). */
export function Callout({ tone = "info", lead, children, className, testId = "callout" }: { tone?: CalloutTone; /** The bold words the note opens with. */ lead?: React.ReactNode; children?: React.ReactNode; className?: string; testId?: string }) {
  const t = TONES[tone];
  const Icon = t.icon;
  return (
    <div role={tone === "bad" ? "alert" : "status"} className={cn("flex items-start gap-3 rounded-xl border px-4 py-3.5 text-[13.5px] leading-[21px] text-foreground", t.box, className)} data-slot="callout" data-tone={tone} data-testid={testId}>
      <Icon aria-hidden className={cn("mt-0.5 size-[18px] shrink-0", t.ink)} />
      <div className="min-w-0 flex-1 break-words">
        {lead && <strong className="font-semibold text-bright">{lead}</strong>}
        {lead && children ? " " : null}
        {children}
      </div>
    </div>
  );
}
