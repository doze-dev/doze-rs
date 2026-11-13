// What the Groups pages share: a sentence of spans, the state word, the lag bar, a call block, a callout, a list of rules.
import { Link } from "@tanstack/react-router";
import { ArrowRightIcon, CheckIcon, CircleXIcon, InfoIcon, TriangleAlertIcon } from "lucide-react";
import type { GroupPhase } from "@/api/types/GroupPhase";
import type { GroupSpan } from "@/api/types/GroupSpan";
import type { Tok } from "@/api/types/Tok";
import { StatusDot } from "@/components/app/status-dot";
import { number } from "@/lib/format";
import { cn } from "@/lib/utils";
import { PHASE_WORD, phaseTone } from "./words";

/** A sentence the server built; a topic is a link to its page. */
export function GroupSpans({ spans }: { spans: GroupSpan[] }) {
  return (
    <>
      {spans.map((s, i) =>
        s.kind === "mono" ? (
          <code key={i} className="font-mono text-[0.92em] text-foreground">
            {s.text}
          </code>
        ) : s.kind === "strong" ? (
          <b key={i} className="font-semibold text-bright">
            {s.text}
          </b>
        ) : s.kind === "topic" ? (
          <Link key={i} to="/topics/$topic" params={{ topic: s.text }} className="font-mono text-link hover:underline">
            {s.text}
          </Link>
        ) : (
          <span key={i}>{s.text}</span>
        ),
      )}
    </>
  );
}

export function PhaseWord({ phase, className }: { phase: GroupPhase; className?: string }) {
  return (
    <StatusDot tone={phaseTone(phase)} className={className} data-testid="group-phase">
      {PHASE_WORD[phase]}
    </StatusDot>
  );
}

/** A share of a whole as a bar, and the number after it in its own slot. */
export function LagBar({ pct, n, className }: { pct: number; n?: number; className?: string }) {
  return (
    <div className={cn("flex min-w-0 items-center gap-3", className)}>
      <div className="h-1.5 min-w-12 flex-1 overflow-hidden rounded-full bg-track" aria-hidden>
        <div className="h-full rounded-full bg-warn" style={{ width: `${Math.max(0, Math.min(100, pct))}%` }} />
      </div>
      {n !== undefined && <span className="w-[8ch] shrink-0 text-right font-mono text-[12.5px] tabular-nums">{number(n)}</span>}
    </div>
  );
}

const TOK: Record<Tok, string> = { plain: "text-foreground", keyword: "text-link", str: "text-good", comment: "text-muted-foreground" };

/** The call a write sends (or sent), one line per entry, as the classic console draws it. */
export function CallBlock({ lines, title, className }: { lines: [Tok, string][][]; title?: string; className?: string }) {
  return (
    <div className={cn("min-w-0 rounded-lg border bg-well p-4", className)} data-testid="call-block">
      {title && <div className="mb-2 text-[11.5px] font-medium tracking-[0.06em] text-muted-foreground uppercase">{title}</div>}
      <pre className="overflow-x-auto font-mono text-[12.5px] leading-[20px] whitespace-pre">
        {lines.map((l, i) => (
          <div key={i}>
            {l.map(([t, s], j) => (
              <span key={j} className={TOK[t]}>
                {s}
              </span>
            ))}
          </div>
        ))}
      </pre>
    </div>
  );
}

export type CalloutTone = "plain" | "warn" | "ok" | "refused";
const CALLOUT: Record<CalloutTone, [string, React.ComponentType<{ className?: string }>, string]> = {
  plain: ["border bg-card", InfoIcon, "text-cool"],
  ok: ["border bg-card", CheckIcon, "text-good"],
  warn: ["border border-warn/30 bg-warn/5", TriangleAlertIcon, "text-warn"],
  refused: ["border border-refused/30 bg-refused/5", CircleXIcon, "text-refused"],
};

export function Callout({ tone = "plain", children, className, ...rest }: { tone?: CalloutTone; children: React.ReactNode; className?: string } & React.HTMLAttributes<HTMLDivElement>) {
  const [box, Icon, color] = CALLOUT[tone];
  return (
    <div {...rest} className={cn("flex gap-3 rounded-xl p-4 text-[13.5px] leading-[21px] text-muted-foreground", box, className)}>
      <Icon className={cn("mt-0.5 size-[18px] shrink-0", color)} aria-hidden />
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

/** Kafka's refusal, in its name, number and words. */
export function RefusalCallout({ code, number: n, message, rule, className }: { code: string; number: number; message: string; rule?: React.ReactNode; className?: string }) {
  return (
    <Callout tone="refused" className={className} role="alert" data-testid="refusal">
      <div className="flex flex-wrap items-center gap-2">
        <StatusDot tone="bad" className="text-refused">
          Refused
        </StatusDot>
        <code className="font-mono text-[12.5px] font-semibold text-bright" data-testid="refusal-code">
          {code} ({n})
        </code>
      </div>
      <p className="mt-2">
        <b className="font-semibold text-bright">Kafka's words:</b> <span data-testid="refusal-words">"{message}"</span>
      </p>
      {rule && (
        <p className="mt-2">
          <b className="font-semibold text-bright">The rule.</b> {rule}
        </p>
      )}
    </Callout>
  );
}

export type RuleRow = { ok?: boolean; blocking?: boolean; title: React.ReactNode; sub?: React.ReactNode; value?: React.ReactNode; here?: boolean };

/** A list of rules or steps: a mark, the line and its explanation, a value at the right. */
export function Rules({ rows, className }: { rows: RuleRow[]; className?: string }) {
  return (
    <ul className={cn("flex flex-col divide-y", className)}>
      {rows.map((r, i) => (
        <li key={i} className="grid grid-cols-[20px_minmax(0,1fr)_auto] items-start gap-3 py-3 first:pt-0 last:pb-0">
          <span className="mt-0.5">
            {r.blocking ? <TriangleAlertIcon className="size-4 text-warn" aria-hidden /> : r.ok ? <CheckIcon className="size-4 text-good" aria-hidden /> : <ArrowRightIcon className="size-4 text-muted-foreground" aria-hidden />}
          </span>
          <div className="min-w-0 text-[13.5px]">
            <div className={cn("text-foreground", r.here && "font-semibold text-bright")}>{r.title}</div>
            {r.sub && <div className="mt-0.5 text-[12.5px] text-muted-foreground">{r.sub}</div>}
          </div>
          <span className="font-mono text-[12.5px] whitespace-nowrap text-muted-foreground tabular-nums">{r.value}</span>
        </li>
      ))}
    </ul>
  );
}

/** A small card with a heading. */
export function Panel({ title, aside, children, className, ...rest }: { title?: React.ReactNode; aside?: React.ReactNode; children: React.ReactNode; className?: string } & React.HTMLAttributes<HTMLElement>) {
  return (
    <section {...rest} className={cn("min-w-0 rounded-xl border bg-card p-6", className)}>
      {(title || aside) && (
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          {title && <h2 className="text-[17px] font-semibold text-bright">{title}</h2>}
          {aside}
        </div>
      )}
      {children}
    </section>
  );
}

/** A key and value list. */
export function Facts({ rows, className }: { rows: [string, React.ReactNode][]; className?: string }) {
  return (
    <dl className={cn("grid grid-cols-[minmax(7.5rem,11rem)_minmax(0,1fr)] gap-x-4 gap-y-2.5 text-[13.5px]", className)}>
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-muted-foreground">{k}</dt>
          <dd className="min-w-0 break-words text-bright">{v}</dd>
        </div>
      ))}
    </dl>
  );
}
