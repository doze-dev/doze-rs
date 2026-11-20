import { CircleAlertIcon, CircleCheckIcon, InfoIcon, type LucideIcon, RefreshCwIcon, TriangleAlertIcon } from "lucide-react";
import { ApiFailure } from "../../lib/api";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";

export type AlertTone = "bad" | "warn" | "info" | "good";

const TONES: Record<AlertTone, { box: string; ink: string; icon: LucideIcon }> = {
  bad: { box: "border-bad/30 bg-bad/5", ink: "text-bad", icon: CircleAlertIcon },
  warn: { box: "border-warn/30 bg-warn/5", ink: "text-warn", icon: TriangleAlertIcon },
  info: { box: "border-cool/30 bg-cool/5", ink: "text-cool", icon: InfoIcon },
  good: { box: "border-good/30 bg-good/5", ink: "text-good", icon: CircleCheckIcon },
};

/** A block that says something happened, where it happened: a title (what), a body (the details and what to do), and the
 * way on. `bad` and `warn` are announced at once (`role="alert"`), `info` and `good` politely (`role="status"`). The tone's
 * colour is on the icon only; the words carry the meaning. */
export function Alert({ tone = "info", title, children, action, icon, className, testId = "alert" }: { tone?: AlertTone; title: string; children?: React.ReactNode; /** Buttons or links under the words. */ action?: React.ReactNode; icon?: LucideIcon; className?: string; testId?: string }) {
  const t = TONES[tone];
  const Icon = icon ?? t.icon;
  return (
    <div role={tone === "bad" || tone === "warn" ? "alert" : "status"} className={cn("flex gap-3 rounded-xl border p-4 text-[13.5px] leading-[21px]", t.box, className)} data-slot="alert" data-tone={tone} data-testid={testId}>
      <Icon aria-hidden className={cn("mt-0.5 size-5 shrink-0", t.ink)} />
      <div className="min-w-0 flex-1 break-words">
        <div className="font-semibold text-bright" data-slot="alert-title">
          {title}
        </div>
        {children && <div className="mt-0.5 text-foreground">{children}</div>}
        {action && <div className="mt-3 flex flex-wrap items-center gap-2">{action}</div>}
      </div>
    </div>
  );
}

/** What an error says for itself: its words, and the code a script or a search can match (an ApiFailure's `code`, with the
 * HTTP status when the server answered). */
export function errorWords(error: unknown): { message: string; code?: string } {
  if (error instanceof ApiFailure) return { message: error.message, code: error.status > 0 ? `${error.code} · HTTP ${error.status}` : error.code };
  if (error instanceof Error && error.message) return { message: error.message };
  return { message: "No reason was given." };
}

/** A fetch that failed, said where its data would have been: what could not be had (`what`, a whole sentence), the server's
 * own words and code, what to do about it (`hint`), and Try again. Never a blank place. */
export function ErrorBlock({ what, error, hint, onRetry, retrying = false, className, testId = "error-block" }: { /** `The services could not be loaded`. */ what: string; error: unknown; /** What the person can do (`Check that doze-aws is still running.`). */ hint?: React.ReactNode; onRetry?: () => void; retrying?: boolean; className?: string; testId?: string }) {
  const { message, code } = errorWords(error);
  return (
    <Alert
      tone="bad"
      title={what}
      className={className}
      testId={testId}
      action={
        onRetry && (
          <Button variant="outline" size="sm" className="bg-card" onClick={onRetry} disabled={retrying} data-testid={`${testId}-retry`}>
            <RefreshCwIcon className={cn(retrying && "animate-spin [animation-duration:1.2s]")} /> Try again
          </Button>
        )
      }
    >
      <p data-slot="error-message">{message}</p>
      {hint && <p className="mt-1 text-muted-foreground" data-slot="error-hint">{hint}</p>}
      {code && <p className="mt-1 font-mono text-[12px] text-muted-foreground" data-slot="error-code">{code}</p>}
    </Alert>
  );
}
