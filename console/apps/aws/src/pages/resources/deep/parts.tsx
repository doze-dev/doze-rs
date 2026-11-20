// Small pieces the resource pages share: a card with a title, a refusal in the service's own words, a line naming the call
// made, a JSON block, a read-only code block, a read's states, a time, and a confirm dialog.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";
import { timeMs } from "../model";

/** A card with a title and, under it, what it is for. */
export function Card({ title, lede, actions, children, className, testId }: { title: React.ReactNode; lede?: React.ReactNode; actions?: React.ReactNode; children?: React.ReactNode; className?: string; testId?: string }) {
  return (
    <section className={cn("rounded-[14px] border bg-card p-6 lg:p-7", className)} data-testid={testId}>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          {lede && <p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children && <div className="mt-5">{children}</div>}
    </section>
  );
}

/** A refusal of `operation`, in the service's words; nothing when there is none. */
export function Refused({ operation, error, testId = "op-error" }: { operation: string; error: unknown; testId?: string }) {
  if (!error) return null;
  const w = errorWords(error);
  return (
    <Alert tone="bad" title={w.code ? `${operation} was refused: ${w.code}` : `${operation} failed`} testId={testId}>
      {w.message}
    </Alert>
  );
}

/** `Calls SendMessage as the caller console; Live shows each one.`, and `more` after it. */
export function Made({ ops, className, more }: { ops: string[]; className?: string; more?: React.ReactNode }) {
  return (
    <p className={cn("text-[12.5px] text-muted-foreground", className)}>
      Calls{" "}
      {ops.map((o, i) => (
        <span key={o}>
          {i > 0 && (i === ops.length - 1 ? " and " : ", ")}
          <span className="font-mono text-foreground">{o}</span>
        </span>
      ))}{" "}
      as the caller <span className="font-mono text-foreground">console</span>; Live shows each one.{more ? <> {more}</> : null}
    </p>
  );
}

/** A JSON value written out, indented, in a scrolling block. */
export function JsonBlock({ value, className, testId }: { value: unknown; className?: string; testId?: string }) {
  const text = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  return (
    <pre className={cn("max-h-80 overflow-auto rounded-[10px] border bg-muted px-4 py-3 font-mono text-[12.5px] leading-5 whitespace-pre-wrap break-all text-foreground", className)} data-testid={testId}>
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
export function CodeBlock({ text, wrap = true, className, testId, max = "max-h-[560px]" }: { text: string; wrap?: boolean; className?: string; testId?: string; max?: string }) {
  return (
    <pre className={cn("overflow-auto rounded-[12px] border bg-well px-5 py-4 font-mono text-[12.5px] leading-[21px] text-foreground", max, wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre", className)} data-testid={testId}>
      {text}
    </pre>
  );
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

/** A dialog that asks before a change that cannot be undone, and shows the refusal if there is one. */
export function Confirm({ open, onOpenChange, title, children, verb, operation, run, pending, error, testId = "confirm-dialog" }: { open: boolean; onOpenChange: (o: boolean) => void; title: string; children: React.ReactNode; verb: string; operation: string; run: () => void; pending: boolean; error: unknown; testId?: string }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[560px]" data-testid={testId}>
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">{title}</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground">{children}</DialogDescription>
        </DialogHeader>
        <p className="text-[12.5px] text-muted-foreground">
          Called as <span className="font-mono">{operation}</span>; Live shows it.
        </p>
        <Refused operation={operation} error={error} testId={`${testId}-error`} />
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="button" variant="destructive" disabled={pending} onClick={run} data-testid={`${testId}-submit`}>
            {pending ? "Working…" : verb}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Open state for a dialog that also forgets its last refusal when closed. */
export function useOpen(reset: () => void): [boolean, (o: boolean) => void] {
  const [open, setOpen] = useState(false);
  return [
    open,
    (o: boolean) => {
      if (!o) reset();
      setOpen(o);
    },
  ];
}

/** A time as a service answered it (seconds or milliseconds since 1970, or ISO 8601 text), said whole in UTC; a dash for none. */
export function at(value: unknown): string {
  if (value === undefined || value === null || value === "") return "–";
  const ms = timeMs(String(value));
  return ms === null ? String(value) : dateTimeUtc(ms);
}
