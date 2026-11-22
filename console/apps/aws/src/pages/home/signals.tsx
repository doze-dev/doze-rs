import { Link } from "@tanstack/react-router";
import { errorWords } from "@doze/ui/components/app/alert";
import { StatusDot, type Tone } from "@doze/ui/components/app/status-dot";
import type { HomeChecks } from "../../api/types/HomeChecks";
import type { Signal } from "../../api/types/Signal";
import type { SignalKind } from "../../api/types/SignalKind";
import { liveSearch } from "../live/search";
import { ResourceTo } from "../resources/value";
import { Section } from "../card";

const TONE: Record<SignalKind, Tone> = {
  "would-be-denied": "warn",
  denied: "bad",
  errors: "bad",
  "not-built": "idle",
  "dead-letters": "warn",
  alarm: "bad",
  "failed-executions": "bad",
  "failed-stack": "bad",
  "not-active": "idle",
};

/** `/live?outcome=error&service=sqs` as a route and its search. */
export function routeOf(href: string): { to: string; search: Record<string, string> } {
  const [to = "/", query = ""] = href.split("?");
  return { to, search: Object.fromEntries(new URLSearchParams(query)) };
}

const CHIP = "inline-flex h-10 items-center gap-2.5 rounded-full border bg-card px-4 text-[13px] transition-colors hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none";

function Chip({ s }: { s: Signal }) {
  const body = (
    <>
      <StatusDot tone={TONE[s.kind]}>{s.title}</StatusDot>
      <span className="max-w-[18rem] truncate text-muted-foreground" title={s.detail}>
        {s.detail}
      </span>
    </>
  );
  if (s.resource)
    return (
      <ResourceTo to={s.resource} className={`${CHIP} text-foreground no-underline hover:no-underline`} testId="signal">
        {body}
      </ResourceTo>
    );
  const r = routeOf(s.href ?? "/live");
  const [, , service, second] = r.to.split("/");
  if (r.to.startsWith("/resources/") && service && second)
    return (
      <Link to="/resources/$service/$second" params={{ service, second }} search={{}} className={CHIP} data-testid="signal" data-kind={s.kind}>
        {body}
      </Link>
    );
  if (r.to === "/access")
    return (
      <Link to="/access" className={CHIP} data-testid="signal" data-kind={s.kind}>
        {body}
      </Link>
    );
  return (
    <Link to="/live" search={liveSearch(r.search)} className={CHIP} data-testid="signal" data-kind={s.kind}>
      {body}
    </Link>
  );
}

/** Needs a look: what the Live record says (refusals, error bursts, operations not built), then what the services apps called
 * hold (a dead-letter queue with messages, an alarm, failed executions, a failed stack). Only real signals; none is said so. */
export function Signals({ live, checks, checksError }: { live?: Signal[]; checks?: HomeChecks; checksError: unknown }) {
  const all = [...(live ?? []), ...(checks?.signals ?? [])];
  const looked = checks?.checked.length ? `Looked in ${checks.checked.join(", ")}: the services apps called in this run, so nothing asleep was woken.` : "No app has called a service whose state is looked at (SQS, CloudWatch, Step Functions, CloudFormation), so none was woken to look.";
  return (
    <Section title="Needs a look" testId="needs-a-look" lede={checks ? looked : "Reading the Live record and the services apps called…"}>
      {live && all.length === 0 ? (
        <p className="text-[13.5px] text-muted-foreground" data-testid="nothing-needs-a-look">
          Nothing: no refusals, no bursts of errors, no operation that is not built{checks?.checked.length ? ", and nothing in the services looked at" : ""}.
        </p>
      ) : (
        <div className="flex flex-wrap gap-3">
          {all.map((s, i) => (
            <Chip key={`${s.kind}-${s.detail}-${i}`} s={s} />
          ))}
        </div>
      )}
      {(checks?.failed.length ?? 0) > 0 && <p className="mt-3 text-[13px] text-muted-foreground">Could not look: {checks?.failed.join("; ")}</p>}
      {checksError != null && <p className="mt-3 text-[13px] text-muted-foreground">The services could not be looked in: {errorWords(checksError).message}</p>}
    </Section>
  );
}
