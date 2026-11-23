// The pieces Home, Access, Lab and Settings share: a titled card, its loading rows, a section heading, a value to copy, and a
// clock that ticks for "how long ago".
import { useEffect, useState } from "react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";

export function Card({ title, lede, children, testId, className, actions }: { title: React.ReactNode; lede?: React.ReactNode; children: React.ReactNode; testId: string; className?: string; actions?: React.ReactNode }) {
  return (
    <section className={cn("min-w-0 rounded-[var(--radius-card)] border bg-card p-6 lg:p-7", className)} aria-labelledby={`${testId}-title`} data-testid={testId}>
      <div className="flex items-start justify-between gap-4">
        <h2 id={`${testId}-title`} className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
          {title}
        </h2>
        {actions}
      </div>
      {lede && <p className="mt-1.5 text-[13.5px] text-muted-foreground">{lede}</p>}
      <div className="mt-5">{children}</div>
    </section>
  );
}

export function Section({ title, lede, children, testId, actions }: { title: string; lede?: React.ReactNode; children: React.ReactNode; testId: string; actions?: React.ReactNode }) {
  return (
    <section className="mb-10" aria-labelledby={`${testId}-title`} data-testid={testId}>
      <div className="flex items-end justify-between gap-4">
        <h2 id={`${testId}-title`} className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
          {title}
        </h2>
        {actions}
      </div>
      {lede && <p className="mt-1.5 max-w-[46rem] text-[13.5px] text-muted-foreground">{lede}</p>}
      <div className="mt-4">{children}</div>
    </section>
  );
}

export function CardSkeleton({ rows }: { rows: number }) {
  return (
    <div aria-busy aria-label="Loading">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex items-center gap-6 border-t py-4 first:border-t-0">
          <Skeleton className="h-3.5 w-24" />
          <Skeleton className="h-3.5 w-48" />
        </div>
      ))}
    </div>
  );
}

/** A value to copy: what it is, the value on one line (cut with an ellipsis, whole on hover), and Copy. */
export function CopyRow({ label, value, testId }: { label: string; value: string; testId: string }) {
  return (
    <div className="flex items-center gap-4 border-t py-4 first:border-t-0 first:pt-0 last:pb-0">
      <div className="min-w-0 flex-1">
        <div className="text-[13px] text-muted-foreground">{label}</div>
        <div className="mt-1 truncate font-mono text-[14.5px] text-bright" title={value} data-testid={testId}>
          {value}
        </div>
      </div>
      <CopyButton text={value} what={`${label} copied`} className="bg-card" />
    </div>
  );
}

/** A block of code with Copy under it. */
export function CodeBlock({ code, what, testId, copyLabel }: { code: string; what: string; testId?: string; copyLabel?: string }) {
  return (
    <div>
      <pre className="overflow-x-auto rounded-xl border bg-well px-5 py-4 font-mono text-[12.5px] leading-[21px] whitespace-pre text-foreground" data-testid={testId}>
        {code}
      </pre>
      <div className="mt-2.5">
        <CopyButton text={code} what={what} label={copyLabel} className="bg-card" />
      </div>
    </div>
  );
}

/** Now, every `ms`: what "how long ago" is measured from while a page is on screen. */
export function useNow(ms = 10_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}
