import { Link } from "@tanstack/react-router";
import { RadioIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { clockUtc } from "@doze/ui/lib/format";
import { useAbout } from "../api/queries";
import { usePrincipals } from "../api/spaces";
import type { CallRecord } from "../api/types/CallRecord";
import type { KeyPrincipal } from "../api/types/KeyPrincipal";
import { Result } from "../pages/live/outcome";
import { callerWords, isConsoleRead, namedKeys, targetKey, tookWords } from "./calls";

function columns(titles: ReadonlyMap<string, string>, names: ReadonlyMap<string, KeyPrincipal>, note?: (c: CallRecord) => string | null): DataColumn<CallRecord>[] {
  return [
    { id: "time", header: "Time", size: 120, accessorKey: "at_ms", meta: { mono: true }, cell: (c) => clockUtc(c.row.original.at_ms) },
    {
      id: "caller",
      header: "Caller",
      size: 260,
      min: 160,
      accessorFn: (r) => callerWords(r, titles, names).label,
      cell: (c) => {
        const w = callerWords(c.row.original, titles, names);
        return (
          <span className="flex min-w-0 items-baseline gap-2" title={w.title} data-testid="call-caller">
            <span className="truncate text-[13px]">{w.label}</span>
            {w.key && <span className="truncate font-mono text-[11.5px] text-muted-foreground">{w.key}</span>}
          </span>
        );
      },
    },
    {
      id: "call",
      header: "Call",
      accessorFn: (r) => r.action ?? "",
      meta: { mono: true },
      cell: (c) => {
        const key = targetKey(c.row.original);
        return (
          <span className="flex min-w-0 items-baseline gap-2">
            <span className="shrink-0 font-mono text-[13px] font-medium text-bright">{c.row.original.action ?? "–"}</span>
            {key && (
              <span className="truncate font-mono text-[11.5px] text-muted-foreground" title={c.row.original.resource ?? undefined} data-testid="call-key">
                {key}
              </span>
            )}
          </span>
        );
      },
    },
    {
      id: "result",
      header: "Result",
      size: note ? 380 : 260,
      min: 140,
      accessorFn: (r) => r.status,
      cell: (c) => {
        const took = tookWords(c.row.original);
        return (
          <span className="flex min-w-0 items-center gap-2">
            <Result call={c.row.original} />
            {took && <span className="truncate text-[12px] text-muted-foreground tabular-nums" data-testid="call-took">{took}</span>}
            {note?.(c.row.original) && <span className="truncate text-[13px]" data-testid="call-note">{note(c.row.original)}</span>}
          </span>
        );
      },
    },
  ];
}

/** How many rows the table shows before it scrolls: the box ends on a row, so no row is cut in half. */
export const CALLS_IN_VIEW = 10;

/** The Recent calls of a resource, newest first: when, who (an IAM user by name, a service by what it made the call for, any other
 * key by its client), what, and how it ended with how long it took. The reads this console makes to draw a page are noise beside
 * an app's calls and its own writes, so they are hidden until asked for; its writes stay. `noun` is what the resource is (`queue`).
 * The one place every kind's recent calls are drawn.
 * Optional: `scope` is what the calls acted on when that is not `this {noun}` (`SNS`); `pin` names the operations whose calls this
 * console made that stay in view even if they are reads (a page that has just revealed a secret shows that call); `limit` cuts the table. */
export function RecentCalls({ calls, noun, title = "Recent calls", className, scope, pin, limit, note }: { calls: readonly CallRecord[]; noun: string; title?: string; className?: string; scope?: string; pin?: readonly string[]; limit?: number; /** What a call's result says beyond its status and time (`1 event, matched order-placed`), where the page knows it. */ note?: (c: CallRecord) => string | null }) {
  const [showReads, setShowReads] = useState(false);
  const what = scope ?? `this ${noun}`;
  const hidden = calls.filter((c) => isConsoleRead(c, pin)).length;
  const shown = (showReads ? calls : calls.filter((c) => !isConsoleRead(c, pin))).slice(0, limit);
  const principals = usePrincipals(useMemo(() => namedKeys(calls), [calls]));
  const about = useAbout().data;
  const cols = useMemo(
    () => columns(new Map((about?.services ?? []).map((s) => [s.name, s.title])), new Map((principals.data?.principals ?? []).map((p) => [p.key, p])), note),
    [about, principals.data, note],
  );
  const more = shown.length - CALLS_IN_VIEW;
  return (
    <section className={className} aria-labelledby="calls-title" data-testid="recent-calls">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 id="calls-title" className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          <p className="mt-1 mb-4 text-[13.5px] text-muted-foreground">
            The last calls in the Live record that acted on {what}, newest first. Times are UTC.
            {hidden > 0 && (
              <span data-testid="calls-console-hidden">
                {" "}
                {hidden} made by this console {hidden === 1 ? "is" : "are"} {showReads ? "listed" : "hidden"}.{" "}
                <button type="button" className="rounded-sm font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => setShowReads(!showReads)} data-testid="calls-console-toggle">
                  {showReads ? "Hide them" : "Show them"}
                </button>
              </span>
            )}
          </p>
        </div>
        <Button asChild variant="outline" size="sm" className="mb-4 bg-card">
          <Link to={"/live" as never}>
            <RadioIcon /> Open Live
          </Link>
        </Button>
      </div>
      <DataTable aria-label={title} data={shown} columns={cols} rowId={(r) => String(r.seq)} maxRows={CALLS_IN_VIEW} empty={hidden > 0 && !showReads ? `Only the reads this console made have acted on ${what} so far.` : `No call in the Live record acted on ${what} yet.`} />
      {more > 0 && (
        <p className="mt-2 text-[12.5px] text-muted-foreground" data-testid="calls-more">
          The newest {CALLS_IN_VIEW} are in view; scroll the table for {more} more.
        </p>
      )}
    </section>
  );
}
