// The group's wire (board group-wire): every request among the rows the recorder keeps that touched the group, newest first,
// with the chips of the classic page (All, Join and sync, Commits, Heartbeats, Refused) and a text filter, all in the address.
// The same table is the "What just happened" dock of the Members tab.
import { getRouteApi, Link } from "@tanstack/react-router";
import { SearchIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useGroupWire } from "@/api/groups";
import type { GroupWireRow } from "@/api/types/GroupWireRow";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { hhmmUtc, number } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Callout } from "./parts";
import { clockMs } from "./words";

const route = getRouteApi("/groups/$group/wire");

const TONE: Record<string, string> = { ok: "text-good", warn: "text-warn", bad: "text-bad", refused: "text-refused" };

function took(us: number): string {
  if (us < 1000) return "<1 ms";
  const ms = Math.floor(us / 1000);
  return ms < 1000 ? `${ms} ms` : `${Math.floor(ms / 1000)}.${Math.floor((ms % 1000) / 100)} s`;
}

const COLUMNS: DataColumn<GroupWireRow>[] = [
  { id: "time", header: "Time", size: 132, accessorFn: (r) => clockMs(r.at_ms), meta: { mono: true } },
  {
    id: "client",
    header: "Client",
    size: 190,
    accessorFn: (r) => r.client,
    cell: ({ row: { original: r } }) => (
      <span className="flex min-w-0 items-center gap-1.5">
        <span className="truncate font-mono text-[12.5px]">{r.client}</span>
        {r.tag && (
          <Badge variant="outline" className="shrink-0 px-1.5 font-mono text-[10.5px]">
            {r.tag}
          </Badge>
        )}
      </span>
    ),
  },
  {
    id: "request",
    header: "Request",
    size: 190,
    accessorFn: (r) => `${r.api} v${r.version}`,
    cell: ({ row: { original: r } }) => (
      <span className="flex min-w-0 items-center gap-1.5">
        <b className="truncate font-mono text-[12.5px] font-medium text-bright">{r.api}</b>
        <span className="font-mono text-[12px] text-muted-foreground">v{r.version}</span>
        {r.count > 1 && <span className="shrink-0 rounded border bg-well px-1 font-mono text-[11px] text-muted-foreground">× {number(r.count)}</span>}
      </span>
    ),
  },
  {
    id: "touched",
    header: "What it touched",
    accessorFn: (r) => [r.target, r.target_sub].filter(Boolean).join(" "),
    cell: ({ row: { original: r } }) => (
      <span className="truncate">
        <span className="font-mono text-[12.5px]">{r.target}</span>
        {r.target_sub && <span className="ml-1.5 text-[12.5px] text-muted-foreground">{r.target_sub}</span>}
      </span>
    ),
  },
  {
    id: "result",
    header: "Result",
    size: 230,
    accessorFn: (r) => `${r.word} ${r.code ? `${r.code_name} (${r.code})` : r.detail}`,
    cell: ({ row: { original: r } }) => (
      <span className="flex min-w-0 items-center gap-1.5" data-testid="wire-result">
        <span className={cn("inline-flex shrink-0 items-center gap-1.5 text-[13px] font-semibold", TONE[r.tone])}>
          <span aria-hidden className="size-2 rounded-full bg-current" />
          {r.word}
        </span>
        <span className="truncate font-mono text-[11.5px] text-muted-foreground">{r.code ? `${r.code_name} (${r.code})` : r.detail}</span>
      </span>
    ),
  },
  { id: "took", header: "Took", size: 84, accessorFn: (r) => took(r.took_us), meta: { numeric: true, mono: true } },
];

export function WireTable({ rows, empty, maxHeight }: { rows: GroupWireRow[]; empty: string; maxHeight?: number }) {
  return <DataTable data={rows} columns={COLUMNS} rowId={(r) => String(r.id)} aria-label="Requests that touched this group" empty={empty} maxHeight={maxHeight} />;
}

/** "What just happened": the newest requests that touched the group, on the Members tab. */
export function WireDock({ id }: { id: string }) {
  const res = useGroupWire(id, "", false, "");
  const rows = useMemo(() => (res.data?.rows ?? []).slice(0, 6), [res.data]);
  return (
    <section className="mt-12" data-testid="wire-dock">
      <div className="mb-3 flex flex-wrap items-end justify-between gap-2">
        <div>
          <h2 className="text-[17px] font-semibold text-bright">What just happened</h2>
          <p className="text-[13.5px] text-muted-foreground">The last requests that touched {id}. Repeats fold into one row.</p>
        </div>
        <Link to="/groups/$group/wire" params={{ group: id }} className="text-[13px] font-medium text-primary hover:underline">
          Open the group's wire
        </Link>
      </div>
      {res.data ? <WireTable rows={rows} empty={`No client request has touched ${id} among the rows the broker kept.`} maxHeight={320} /> : <Skeleton className="h-48 rounded-xl" />}
    </section>
  );
}

const CHIPS = [
  { value: "all", label: "All" },
  { value: "join", label: "Join and sync" },
  { value: "commits", label: "Commits" },
  { value: "heartbeats", label: "Heartbeats" },
  { value: "refused", label: "Refused" },
] as const;

export function GroupWirePage() {
  const { group: id } = route.useParams();
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const q = search.q ?? "";
  const res = useGroupWire(id, search.kind ?? "", search.refused === 1, q);
  const [text, setText] = useState(q);
  useEffect(() => {
    if (text.trim() === q) return;
    const t = setTimeout(() => void navigate({ search: (s) => ({ ...s, q: text.trim() || undefined }), replace: true, resetScroll: false }), 200);
    return () => clearTimeout(t);
  }, [text, q, navigate]);
  const chip = search.refused === 1 ? "refused" : (search.kind ?? "all");
  const setChip = (v: string) => {
    if (!v) return;
    void navigate({
      search: (s) => ({ ...s, kind: v === "join" || v === "commits" || v === "heartbeats" ? v : undefined, refused: v === "refused" ? 1 : undefined }),
      replace: true,
      resetScroll: false,
    });
  };
  const d = res.data;
  const refusedRow = d?.rows.find((r) => r.refused);
  return (
    <div data-testid="group-wire">
      <p className="mb-4 max-w-[52rem] text-[14px] text-muted-foreground">
        Every client request that touched {id} among the rows the broker kept, newest first. Heartbeats fold into one row; a refusal stays on its own row. The console's own requests are not listed.
      </p>
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <ToggleGroup type="single" variant="outline" value={chip} onValueChange={setChip} aria-label="Which requests" data-testid="wire-chips">
          {CHIPS.map((c) => (
            <ToggleGroupItem key={c.value} value={c.value} className="px-3 data-[state=on]:bg-accent data-[state=on]:text-bright">
              {c.label}
              {c.value === "all" && d && <span className="ml-1 font-mono text-[11.5px] text-muted-foreground tabular-nums">{number(d.all)}</span>}
              {c.value === "refused" && d && <span className="ml-1 font-mono text-[11.5px] text-muted-foreground tabular-nums">{number(d.refused)}</span>}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <div className="relative ml-auto w-72 max-w-full">
          <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
          <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="Filter by member or client" aria-label="Filter by member or client" className="bg-card pl-9" data-testid="wire-filter" />
        </div>
      </div>
      {d ? <WireTable rows={d.rows} empty={`No request that touched ${id} matches.`} maxHeight={640} /> : <Skeleton className="h-80 rounded-xl" />}
      <div className="mt-3 flex flex-wrap gap-x-4 text-[13px] text-muted-foreground tabular-nums">
        <span>
          Among the last <span className="inline-block min-w-[5ch] text-right font-mono">{d ? number(d.kept) : "–"}</span> rows the broker kept, back to <span className="inline-block min-w-[5ch] font-mono">{d?.oldest_ms ? hhmmUtc(d.oldest_ms) : "none"}</span> UTC.
        </span>
        <span>Kept in memory and emptied at every restart.</span>
      </div>
      <div className="mt-6 min-h-[64px]">
        {refusedRow && (
          <Callout tone="refused" data-testid="wire-refusal">
            <b className="font-semibold text-bright">
              {refusedRow.code_name} ({refusedRow.code})
            </b>{" "}
            at <time className="font-mono">{clockMs(refusedRow.at_ms)}</time>: {refusedRow.detail || "refused."}
          </Callout>
        )}
      </div>
    </div>
  );
}
