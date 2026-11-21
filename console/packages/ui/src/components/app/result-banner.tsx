import { CircleAlertIcon, CircleCheckIcon, InfoIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";

export type ResultTone = "good" | "warn" | "info" | "bad";

const TONES = {
  good: { box: "border-good/30 bg-good/10", ink: "text-good", icon: CircleCheckIcon },
  warn: { box: "border-warn/30 bg-warn/10", ink: "text-warn", icon: TriangleAlertIcon },
  info: { box: "border-cool/30 bg-cool/10", ink: "text-cool", icon: InfoIcon },
  bad: { box: "border-bad/30 bg-bad/10", ink: "text-bad", icon: CircleAlertIcon },
} as const;

/** What a write did, said where the person is looking (it replaces a toast and a silent navigation): the result in a bold
 * title (`Queue stock-moves.fifo created.`), what the call returned (`CreateQueue returned its URL`), what is left (`the
 * dead-letter queue stays`), and the way on. One line when it can be; it wraps, never truncates. `onDismiss` adds a close
 * button (a result that is still true needs none). */
export function ResultBanner({ tone = "good", title, returned, left, actions, onDismiss, className, testId = "result-banner" }: { tone?: ResultTone; title: string; /** What the call returned. */ returned?: React.ReactNode; /** What is left over (kept, queued, still to do). */ left?: React.ReactNode; actions?: React.ReactNode; onDismiss?: () => void; className?: string; testId?: string }) {
  const t = TONES[tone];
  const Icon = t.icon;
  return (
    <div role={tone === "bad" || tone === "warn" ? "alert" : "status"} className={cn("flex items-start gap-3 rounded-xl border px-4 py-3.5 text-[14px] leading-[22px]", t.box, className)} data-slot="result-banner" data-tone={tone} data-testid={testId}>
      <Icon aria-hidden className={cn("mt-[3px] size-[18px] shrink-0", t.ink)} />
      <div className="min-w-0 flex-1 break-words text-foreground">
        <strong className="font-semibold text-bright">{title}</strong>
        {returned && <> {returned}</>}
        {left && <> {left}</>}
        {actions && <div className="mt-2 flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {onDismiss && (
        <Button type="button" variant="ghost" size="icon-xs" className="-mr-1 shrink-0" onClick={onDismiss} aria-label="Dismiss" data-testid={`${testId}-dismiss`}>
          <XIcon />
        </Button>
      )}
    </div>
  );
}
