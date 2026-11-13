// One rule (`/lab?rule=<id>`): a drawer over the Lab with its sentence, figures as tiles (hits, last hit, time left counted down
// locally, the delay or the error) and the requests it touched, each with the Lab tag.
import { Trash2Icon } from "lucide-react";
import { useEffect, useState } from "react";
import { useLabRule } from "@/api/r4";
import type { LabHit } from "@/api/types/LabHit";
import type { LabRule } from "@/api/types/LabRule";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { RefreshControl } from "@/components/app/refresh-control";
import { StatTile } from "@/components/app/stat";
import { StatusDot } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { clockUtc, hhmmUtc, number } from "@/lib/format";
import { LabRuleBadge } from "./badge";

export const END_WORDS: Record<NonNullable<LabRule["ended"]>, string> = {
  expired: "Ended on its own",
  spent: "Ended after its last hit",
  cleared: "Removed",
  replaced: "Replaced by a newer rule",
};

/** `7:42` left: a deadline counted down in its own slot, each second, with no request (CON-812). */
export function Remaining({ endsMs, nowMs }: { endsMs: number; nowMs: number }) {
  // The server's clock at the answer, carried forward by this browser's own clock.
  const [skew] = useState(() => nowMs - Date.now());
  const [now, setNow] = useState(() => Date.now() + skew);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now() + skew), 1000);
    return () => clearInterval(t);
  }, [skew]);
  const left = Math.max(0, Math.floor((endsMs - now) / 1000));
  const m = Math.floor(left / 60);
  const s = left % 60;
  return (
    <time dateTime={new Date(endsMs).toISOString()} className="tabular-nums" data-testid="remaining">
      {left === 0 ? "0:00" : `${m}:${String(s).padStart(2, "0")}`}
    </time>
  );
}

const hitColumns: DataColumn<LabHit>[] = [
  { accessorKey: "at_ms", header: "Time", size: 100, cell: (c) => <span className="font-mono text-[12.5px]">{clockUtc(c.row.original.at_ms)}</span> },
  { accessorKey: "client", header: "Client", size: 130, cell: (c) => <span title={c.row.original.client}>{c.row.original.client}{c.row.original.tag ? ` (${c.row.original.tag})` : ""}</span> },
  { id: "request", header: "Request", accessorFn: (h) => `${h.api} v${h.version}${h.transactional ? " (txn)" : ""} · ${h.target}`, meta: { mono: true } },
  {
    id: "result",
    header: "Result",
    size: 150,
    accessorFn: (h) => (h.code === 0 ? "Done" : h.code_name),
    cell: (c) => (
      <span title={c.row.original.detail}>
        <StatusDot tone={c.row.original.code === 0 && c.row.original.class !== "closed" ? "ok" : "bad"} className="font-medium">
          {c.row.original.class === "closed" ? "Closed" : c.row.original.code === 0 ? "Done" : c.row.original.code_name}
        </StatusDot>
      </span>
    ),
  },
  { id: "took", header: "Took", size: 80, accessorFn: (h) => `${number(Math.round(h.took_us / 1000))} ms`, meta: { numeric: true } },
];

export function RuleSheet({ id, onClose, onSwitch, onRemove }: { id: string | undefined; onClose: () => void; onSwitch: (id: number, on: boolean) => void; onRemove: (id: number) => void }) {
  const q = useLabRule(id);
  const v = q.data && String(q.data.rule.id) === id ? q.data : undefined;
  const r = v?.rule;
  return (
    <Sheet open={!!id} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full overflow-y-auto sm:max-w-4xl" data-testid="lab-rule-sheet">
        <SheetHeader>
          <SheetTitle className="flex flex-wrap items-center gap-3 pr-8 text-[20px] font-medium">
            {r?.name ?? `Rule ${id}`}
            {r && (r.ended ? <StatusDot tone="idle">{END_WORDS[r.ended]}</StatusDot> : r.enabled ? <StatusDot tone="ok">Active</StatusDot> : <StatusDot tone="idle">Switched off</StatusDot>)}
          </SheetTitle>
          <SheetDescription>{r ? r.sentence : "One Lab rule and the requests it touched."}</SheetDescription>
          {/* The sheet keeps up as the page does: the interval chosen for every page while it is open, and its own Refresh. */}
          <RefreshControl className="mt-2" updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} />
        </SheetHeader>
        <div className="flex min-w-0 flex-col gap-6 px-4 pb-8">
          {q.error && !v ? (
            <p role="alert" className="text-[13.5px] text-bad">
              {q.error.message}
            </p>
          ) : !v || !r ? (
            <Skeleton className="h-80 w-full" />
          ) : (
            <>
              {!r.ended && (
                <div className="flex flex-wrap items-center gap-4 rounded-lg border bg-card p-3">
                  <label className="flex items-center gap-2 text-[13.5px]">
                    <Switch checked={r.enabled} onCheckedChange={(on) => onSwitch(r.id, on)} aria-label={`Rule ${r.id} on`} data-testid="sheet-switch" />
                    {r.enabled ? "On: it fires on the requests it matches" : "Off: it matches nothing until switched on"}
                  </label>
                  <Button variant="outline" size="sm" className="ml-auto bg-card text-bad" onClick={() => onRemove(r.id)} data-testid="sheet-remove">
                    <Trash2Icon /> Remove rule
                  </Button>
                </div>
              )}
              <dl className="flex flex-wrap gap-3" data-slot="stat-strip">
                {[
                  { label: "Hits", value: number(r.hits), unit: `of ${number(r.matched)} matched · ${r.share}`, ch: 6 },
                  { label: "Last hit", value: r.last_hit_ms ? clockUtc(r.last_hit_ms) : "none yet", unit: "UTC", mono: true, ch: 8 },
                  r.ended
                    ? { label: "Ended", value: hhmmUtc(r.ends_ms), unit: `started ${hhmmUtc(r.started_ms)} UTC`, mono: true, ch: 6 }
                    : null,
                  r.delay_ms !== null ? { label: "Added delay", value: number(r.delay_ms), unit: "ms each", ch: 6 } : r.code ? { label: "Answers", value: r.code[1], unit: `error ${r.code[0]}`, mono: true, ch: 12 } : { label: "Effect", value: "drop", unit: "closes the connection", ch: 6 },
                ].map((s, i) =>
                  s ? (
                    <StatTile key={s.label} stat={s} className="flex-1 basis-[150px]" />
                  ) : (
                    <div key={i} className="min-w-0 flex-1 basis-[150px] rounded-xl border bg-card px-5 py-4" data-slot="stat" data-testid="tile-remaining">
                      <dt className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">Time left</dt>
                      <dd className="mt-1 truncate text-[22px] leading-8 font-medium tracking-tight text-bright tabular-nums" style={{ minWidth: "6ch" }}>
                        <Remaining endsMs={r.ends_ms} nowMs={v.now_ms} />
                      </dd>
                      <div className="truncate text-[12.5px] text-muted-foreground">ends {hhmmUtc(r.ends_ms)} UTC</div>
                    </div>
                  ),
                )}
              </dl>
              <div className="min-w-0">
                <h3 className="mb-1 text-[16px] font-medium text-bright">The last hits</h3>
                <p className="mb-3 text-[13px] text-muted-foreground">The same rows as on Live, only the ones this rule touched{v.more_hits ? "; the newest eight" : ""}.</p>
                <DataTable
                  data={v.hits}
                  columns={[...hitColumns, { id: "tag", header: () => <span className="sr-only">Lab</span>, size: 64, enableSorting: false, cell: (c) => <LabRuleBadge rule={r.id} name={r.name} did={c.row.original.did} /> }]}
                  rowId={(h) => String(h.id)}
                  aria-label="Requests the rule touched"
                  maxHeight={420}
                  empty="No request has matched yet. Use the app the rule names, and its requests show here."
                />
              </div>
              {r.first && (
                <p className="text-[13px] text-muted-foreground" data-testid="first-hit">
                  The first hit: {r.first.client}, {r.first.api} v{r.first.version}
                  {r.first.target ? ` to ${r.first.target}` : ""} at {clockUtc(r.first.at_ms)} UTC.
                </p>
              )}
              {r.clients.length > 0 && (
                <p className="text-[13px] text-muted-foreground">
                  Apps it touched: <span className="font-mono text-foreground">{r.clients.join(", ")}</span>
                  {r.more_clients > 0 ? ` and ${r.more_clients} more` : ""}.
                </p>
              )}
              <p className="rounded-lg border bg-well p-3 text-[13px] text-muted-foreground">
                <b className="font-semibold text-foreground">Nothing is wrong with the broker.</b> The rule only changes the answer, so no record changes and no offset moves. When it ends, requests behave as before with no restart.
              </p>
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}
