// The Topics list (boards topics-list, topics-filtered, topics-none; classic pages/topics/list.rs): the filter and the
// internal toggle are in the address; the table is a DataTable with stable ids; the state is chosen by the data.
import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { InfoIcon, PlusIcon, Rows3Icon, SearchIcon, XIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useShell } from "@/api/queries";
import { topicKeys, useTopics } from "@/api/topics";
import type { TopicRow } from "@/api/types/TopicRow";
import type { TopicsResponse } from "@/api/types/TopicsResponse";
import { CopyButton } from "@/components/app/copy-button";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { EmptyState } from "@/components/app/empty-state";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { type Stat, StatStrip } from "@/components/app/stat";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { number } from "@/lib/format";
import { bytes, dateTime, retention, stamp } from "@/lib/kafka";
import type { TopicsSearch } from "@/lib/topic-search";
import { PageError } from "../page-error";
import { useMinWidth } from "../r4/bits";
import { TopicsOverlays } from "./overlays";
import { policyWord } from "./words";

/** The small line under a topic's name (classic `sub_line`). */
export function subLine(r: TopicRow): string | null {
  if (r.internal) return "read only · the group coordinator's name for where commits live";
  if (r.empty) return "empty: no records yet";
  const parts: string[] = [];
  if (r.segments !== null && r.segments > 1) parts.push(`${r.segments} segments`);
  if (r.retention_ms !== null) parts.push(`retention ${retention(r.retention_ms)}`);
  if (r.read_by.length) parts.push(`read by ${r.read_by.join(", ")}`);
  return parts.length ? parts.join(" · ") : null;
}

function columns(now: number, wide: boolean): DataColumn<TopicRow>[] {
  return [
    {
      id: "name",
      accessorKey: "name",
      header: "Name",
      cell: ({ row: { original: r } }) => {
        const sub = subLine(r);
        return (
          <div className="flex min-w-0 flex-col justify-center gap-0.5">
            <div className="flex min-w-0 items-center gap-2">
              {r.internal ? (
                <>
                  <span className="truncate font-mono text-[13.5px] text-bright">{r.name}</span>
                  <Badge variant="outline" className="font-mono text-[11px]">
                    internal
                  </Badge>
                </>
              ) : (
                <Link to="/topics/$topic/messages" params={{ topic: r.name }} className="max-w-full shrink-0 truncate font-mono text-[13.5px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="topic-link">
                  {r.name}
                </Link>
              )}
              {/* The name comes first: a state beside it gives way (an ellipsis, the whole word on hover). */}
              {r.offline > 0 && <StatusDot tone="bad" className="min-w-0 overflow-hidden" title="Offline">Offline</StatusDot>}
              {r.txn_open && <StatusDot tone="warn" className="min-w-0 overflow-hidden" title="Transaction open"><span className="truncate">Transaction open</span></StatusDot>}
            </div>
            {sub && (
              <span className="truncate text-[12.5px] text-muted-foreground" title={sub}>
                {sub}
              </span>
            )}
          </div>
        );
      },
    },
    { accessorKey: "partitions", header: "Partitions", size: 122, meta: { numeric: true, mono: true } },
    {
      id: "cleanup",
      accessorFn: (r) => policyWord(r.cleanup),
      header: wide ? "Cleanup policy" : "Cleanup",
      size: wide ? 164 : 112,
      cell: (c) => (
        <Badge variant="outline" className="font-mono text-[11.5px]">
          {c.getValue<string>()}
        </Badge>
      ),
    },
    {
      id: "bytes",
      accessorFn: (r) => r.bytes ?? -1,
      header: wide ? "Size on disk" : "Size",
      size: wide ? 136 : 104,
      meta: { numeric: true, mono: true },
      cell: ({ row: { original: r } }) => (r.internal ? <span className="text-muted-foreground">not a log</span> : r.bytes === null ? "unknown" : bytes(r.bytes)),
    },
    {
      id: "last",
      accessorFn: (r) => r.last_write_ms ?? -1,
      header: "Last write",
      size: wide ? 128 : 116,
      meta: { numeric: true, mono: true },
      cell: ({ row: { original: r } }) => (r.internal ? <span className="text-muted-foreground">none</span> : r.last_write_ms !== null && !r.empty ? <span title={dateTime(r.last_write_ms)}>{stamp(r.last_write_ms, now)}</span> : "never"),
    },
  ];
}

function stats(r: TopicsResponse): Stat[] {
  const v = r.view;
  const out: Stat[] = [
    { label: "Topics", value: number(v.total), ch: 5 },
    { label: "Partitions", value: number(v.partitions), ch: 5 },
  ];
  if (v.newest) {
    out.push({ label: "Newest write", value: stamp(v.newest[1], v.now_ms), mono: true, ch: 11, unit: "UTC", tip: dateTime(v.newest[1]) });
    out.push({ label: "Written to", value: v.newest[0], mono: true, ch: 20 });
  } else {
    out.push({ label: "Newest write", value: "none yet", mono: true, ch: 11 });
    out.push({ label: "Written to", value: "none", mono: true, ch: 20 });
  }
  return out;
}

function InternalNote({ shown }: { shown: boolean }) {
  return (
    <div className="mt-6 flex gap-3 rounded-xl border bg-card p-5 text-[13.5px] leading-[21px]" data-testid="internal-note">
      <InfoIcon className="mt-0.5 size-4.5 shrink-0 text-cool" aria-hidden />
      <div className="min-w-0 text-muted-foreground">
        <p>
          <b className="font-semibold text-bright">{shown ? "The internal topic is shown." : "One internal topic is hidden."}</b> Turn on <b className="font-semibold text-foreground">Show internal</b> and{" "}
          <code className="font-mono text-foreground">__consumer_offsets</code> joins the list.
        </p>
        <p className="mt-2">
          <b className="font-semibold text-foreground">Why it is read only.</b> <code className="font-mono text-foreground">__consumer_offsets</code> is Kafka's own name for the place the group coordinator keeps each group's committed
          offsets, spread over 50 partitions. This broker keeps commits in its own store and answers Kafka's offset calls from it, so there is no log here to read, produce to, delete from, or change. The name is shown because Kafka
          clients and tools know it; the 50 partitions decide which coordinator slot a group lands in. Look at a group's commits on its Lag tab instead:{" "}
          <Link to="/groups" className="font-medium text-link hover:underline">
            open Groups
          </Link>
          .
        </p>
      </div>
    </div>
  );
}

function NoTopics({ listen }: { listen: string }) {
  const cmd = `echo '{"order_id":"ord-88412"}' | kcat -P -b ${listen} -t orders -k ord-88412`;
  const navigate = useNavigate({ from: "/topics" });
  return (
    <div data-testid="topics-none">
      <EmptyState
        icon={Rows3Icon}
        title="This broker holds no topics"
        action={
          <Button onClick={() => void navigate({ search: (s) => ({ ...s, dialog: "create" }) })}>
            <PlusIcon /> Create topic
          </Button>
        }
      >
        A topic is a named log of records, split into partitions. Create one here, or let a client make it: <code className="font-mono">--auto-create-topics</code> is on, so the first request for a name that does not exist creates it with{" "}
        <code className="font-mono">--default-partitions</code> (1).
      </EmptyState>
      <section className="mt-8">
        <h2 className="text-[17px] font-semibold text-bright">Or from your app</h2>
        <p className="mt-1 text-[13.5px] text-muted-foreground">Writing to a topic that does not exist creates it, and the console shows it here as soon as the request lands.</p>
        <pre className="mt-3 overflow-x-auto rounded-xl border bg-well p-4 font-mono text-[12.5px] text-foreground">{cmd}</pre>
        <div className="mt-3 flex items-center gap-3">
          <CopyButton text={cmd} what="Command copied" />
          <span className="text-[12.5px] text-muted-foreground">
            To <code className="font-mono">{listen}</code>, the listener
          </span>
        </div>
      </section>
    </div>
  );
}

/** The name filter: types into the address (replacing the entry) a beat after the last key, so the list follows the typing. */
function useDebouncedFilter(value: string, set: (v: string) => void) {
  const [text, setText] = useState(value);
  const [prev, setPrev] = useState(value);
  // A Back or a Clear changes the address: the field follows it.
  if (value !== prev) {
    setPrev(value);
    setText(value);
  }
  useEffect(() => {
    if (text === value) return;
    const t = setTimeout(() => set(text), 180);
    return () => clearTimeout(t);
  }, [text, value, set]);
  return [text, setText] as const;
}

export function TopicsPage() {
  const search = useSearch({ from: "/topics" });
  const navigate = useNavigate({ from: "/topics" });
  const q = search.q ?? "";
  const internal = !!search.internal;
  const r = useTopics(q, internal);
  const qc = useQueryClient();
  const shell = useShell();
  const setSearch = useCallback((patch: Partial<TopicsSearch>) => void navigate({ search: (s) => ({ ...s, ...patch }), replace: true, resetScroll: false }), [navigate]);
  const setQ = useCallback((v: string) => setSearch({ q: v.trim() ? v : undefined }), [setSearch]);
  const [text, setText] = useDebouncedFilter(q, setQ);
  const now = r.data?.view.now_ms ?? 0;
  const wide = useMinWidth(1280);
  const cols = useMemo(() => columns(now, wide), [now, wide]);
  const refresh = useCallback(() => void qc.invalidateQueries({ queryKey: topicKeys.all }), [qc]);
  const d = r.data;
  const shownRows = d ? d.view.rows.filter((x) => !x.internal).length : 0;
  return (
    <>
      <PageHeader
        title="Topics"
        sub="Where records live. Every topic is a log: open one to read it, produce to it, or see what the broker did to it."
        refresh={<RefreshControl updatedAt={r.dataUpdatedAt} fetching={r.isFetching} onRefresh={refresh} />}
        actions={
          <Button onClick={() => void navigate({ search: (s) => ({ ...s, dialog: "create" }) })} data-testid="act-create">
            <PlusIcon /> Create topic
          </Button>
        }
      />
      {r.error && !d ? (
        <PageError error={r.error} onRetry={() => void r.refetch()} />
      ) : !d ? (
        <div className="flex flex-col gap-4">
          <Skeleton className="h-[106px] w-full rounded-xl" />
          <Skeleton className="h-10 w-96" />
          <Skeleton className="h-80 w-full rounded-xl" />
        </div>
      ) : d.state === "none" ? (
        <NoTopics listen={shell.data?.address ?? ""} />
      ) : (
        <div data-testid="topics-page" data-state={d.state}>
          <StatStrip stats={stats(d)} />
          <div className="mt-6 mb-4 flex flex-wrap items-center gap-3" role="search">
            <div className="relative w-80 max-w-full">
              <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
              <Input value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Escape" && text && (e.preventDefault(), setText(""))} placeholder="Filter topics by name" aria-label="Filter topics by name" className="bg-card pr-9 pl-9" data-testid="topics-filter" spellCheck={false} autoComplete="off" />
              {text && (
                <button type="button" aria-label="Clear the filter" onClick={() => setText("")} className="absolute top-1/2 right-2 -translate-y-1/2 rounded-sm p-1 text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
                  <XIcon className="size-3.5" />
                </button>
              )}
            </div>
            <div className="flex h-9 items-center gap-2.5 rounded-md border bg-card px-3">
              <Switch id="show-internal" checked={internal} onCheckedChange={(on) => setSearch({ internal: on ? true : undefined })} data-testid="show-internal" />
              <Label htmlFor="show-internal" className="cursor-pointer text-[13.5px] font-normal">
                Show internal
              </Label>
            </div>
          </div>
          {d.state === "filtered" ? (
            <EmptyState
              icon={SearchIcon}
              title={`No topic name contains ${d.view.filter}`}
              className="min-h-72"
              action={
                <>
                  <Button variant="outline" onClick={() => setText("")} data-testid="clear-filter">
                    Clear filter
                  </Button>
                  <Button onClick={() => void navigate({ search: (s) => ({ ...s, dialog: "create", name: d.view.filter }) })}>
                    <PlusIcon /> Create <span className="font-mono">{d.view.filter}</span>
                  </Button>
                </>
              }
            >
              The filter looks at the {number(d.view.total)} topic names; internal topics are hidden. Names are matched as typed, so <code className="font-mono">pick-lists</code> will not match <code className="font-mono">pick_lists</code>.
            </EmptyState>
          ) : (
            <>
              <DataTable data={d.view.rows} columns={cols} rowId={(x) => x.name} aria-label="Topics" rowHeight={60} maxHeight={720} empty="No topic matches." />
              <p className="mt-3 text-right text-[13px] text-muted-foreground tabular-nums" data-testid="topics-count">
                {d.view.truncated ? `First ${number(shownRows)} of ${number(d.view.total)} topics, by name` : `${number(shownRows)} of ${number(q ? shownRows : d.view.total)} topics`}
              </p>
              <InternalNote shown={internal} />
            </>
          )}
        </div>
      )}
      <TopicsOverlays />
    </>
  );
}
