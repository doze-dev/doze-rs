// What the deep pages share: a titled card, the line that names the call a section was read with, a read-only code block, the
// error of a read, and a time as the console writes times.
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";

/** A card with a title, a sentence under it, and its actions on the right. */
export function Section({ title, lede, actions, children, className, testId }: { title: React.ReactNode; lede?: React.ReactNode; actions?: React.ReactNode; children?: React.ReactNode; className?: string; testId?: string }) {
  return (
    <section className={cn("rounded-[14px] border bg-card p-6 lg:p-7", className)} data-testid={testId}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          {lede && <p className="mt-1 max-w-[760px] text-[13.5px] leading-[21px] text-muted-foreground">{lede}</p>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children && <div className="mt-5">{children}</div>}
    </section>
  );
}

/** `Read with GetLogEvents, as the caller console.` */
export function ReadWith({ ops, more }: { ops: string[]; more?: React.ReactNode }) {
  return (
    <p className="mt-3 text-[12.5px] leading-[18px] text-muted-foreground">
      {ops.length > 0 && (
        <>
          {ops.length > 1 ? "Calls " : "Called with "}
          {ops.map((o, i) => (
            <span key={o}>
              {i > 0 && ", "}
              <span className="font-mono text-foreground">{o}</span>
            </span>
          ))}{" "}
          as the caller <span className="font-mono text-foreground">console</span>; Live shows {ops.length > 1 ? "them" : "it"}.
        </>
      )}{" "}
      {more}
    </p>
  );
}

/** A read-only block of code or text, with its own scroll; wrapped or not. */
export function CodeBlock({ text, wrap = true, className, testId, max = "max-h-[560px]" }: { text: string; wrap?: boolean; className?: string; testId?: string; max?: string }) {
  return (
    <pre className={cn("overflow-auto rounded-[12px] border bg-well px-5 py-4 font-mono text-[12.5px] leading-[21px] text-foreground", max, wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre", className)} data-testid={testId}>
      {text}
    </pre>
  );
}

/** `text` indented when it is JSON, as it is otherwise. */
export function pretty(text: string | undefined | null): string {
  if (!text) return "";
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

/** A time the services give as seconds (a number), milliseconds, or ISO text, written as the console writes times (UTC). */
export function when(t: number | string | undefined | null): string {
  if (t === undefined || t === null || t === "") return "–";
  const ms = typeof t === "number" ? (t > 1e12 ? t : t * 1000) : Date.parse(t);
  return Number.isNaN(ms) ? String(t) : dateTimeUtc(ms);
}

/** A read's states: loading, refused (the service's words, and Retry), or its body. */
export function Loaded<T>({ q, what, children, height = "h-40" }: { q: { data: T | undefined; error: unknown; isFetching: boolean; refetch: () => unknown }; what: string; children: (data: T) => React.ReactNode; height?: string }) {
  if (q.data !== undefined) return <>{children(q.data)}</>;
  if (q.error) {
    const w = errorWords(q.error);
    return (
      <Alert tone="bad" title={`${what} could not be read${w.code ? `: ${w.code}` : ""}`} testId="deep-error" action={<Button variant="outline" size="sm" className="bg-card" onClick={() => void q.refetch()} disabled={q.isFetching}>{q.isFetching ? "Retrying…" : "Retry"}</Button>}>
        {w.message}
      </Alert>
    );
  }
  return <Skeleton className={cn("rounded-[12px]", height)} data-testid="deep-loading" />;
}

/** The words a refused write is shown with. */
export function Refused({ error, op }: { error: unknown; op: string }) {
  if (!error) return null;
  const w = errorWords(error);
  return (
    <Alert tone="bad" title={w.code ? `${op} was refused: ${w.code}` : `${op} failed`} testId="write-error">
      {w.message}
    </Alert>
  );
}
