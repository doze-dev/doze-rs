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
import { isConsole } from "../pages/live/search";
import { callerWords, namedKeys, tookWords } from "./calls";

function columns(titles: ReadonlyMap<string, string>, names: ReadonlyMap<string, KeyPrincipal>): DataColumn<CallRecord>[] {
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
    { id: "call", header: "Call", accessorFn: (r) => r.action ?? "", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.action ?? "–"}</span> },
    {
      id: "result",
      header: "Result",
      size: 260,
      min: 140,
      accessorFn: (r) => r.status,
      cell: (c) => {
        const took = tookWords(c.row.original);
        return (
          <span className="flex min-w-0 items-center gap-2">
            <Result call={c.row.original} />
            {took && <span className="truncate text-[12px] text-muted-foreground tabular-nums" data-testid="call-took">{took}</span>}
          </span>
        );
      },
    },
  ];
}

/** The Recent calls of a resource, newest first: when, who (an IAM user by name, a service by what it made the call for, any other
 * key by its client), what, and how it ended with how long it took. This console's own calls are noise beside an app's, hidden
 * unless asked for, as on Live. `noun` is what the resource is (`queue`). The one place every kind's recent calls are drawn. */
export function RecentCalls({ calls, noun, title = "Recent calls", className }: { calls: readonly CallRecord[]; noun: string; title?: string; className?: string }) {
  const [mine, setMine] = useState(false);
  const theirs = calls.filter((c) => !isConsole(c));
  const hidden = calls.length - theirs.length;
  const principals = usePrincipals(useMemo(() => namedKeys(calls), [calls]));
  const about = useAbout().data;
  const cols = useMemo(
    () => columns(new Map((about?.services ?? []).map((s) => [s.name, s.title])), new Map((principals.data?.principals ?? []).map((p) => [p.key, p]))),
    [about, principals.data],
  );
  return (
    <section className={className} aria-labelledby="calls-title" data-testid="recent-calls">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 id="calls-title" className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          <p className="mt-1 mb-4 text-[13.5px] text-muted-foreground">
            The last calls in the Live record that acted on this {noun}, newest first. Times are UTC.
            {(hidden > 0 || mine) && (
              <span data-testid="calls-console-hidden">
                {" "}
                {mine ? "This console's own are shown." : `${hidden} made by this console ${hidden === 1 ? "is" : "are"} hidden.`}{" "}
                <button type="button" className="rounded-sm font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => setMine(!mine)} data-testid="calls-console-toggle">
                  {mine ? "Hide them" : "Show"}
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
      <DataTable aria-label={title} data={mine ? [...calls] : theirs} columns={cols} rowId={(r) => String(r.seq)} maxHeight={420} empty={hidden > 0 && !mine ? `Only this console has acted on this ${noun} so far.` : `No call in the Live record acted on this ${noun} yet.`} />
    </section>
  );
}
