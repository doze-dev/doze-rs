// A topic's Messages (boards topic-detail, topic-messages-filtered, topic-messages-empty, topic-readcommitted, topic-offline;
// classic pages/topics/messages.rs, bar.rs, table.rs, offline.rs). Every filter is in the address; the toolbar is ONE layout
// that never moves; the table is a DataTable with stable ids; a record opens in a routed Sheet; the Live tail is opt-in.
import { useIsFetching, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useParams, useSearch } from "@tanstack/react-router";
import { ArrowUpIcon, CalendarClockIcon, CircleAlertIcon, InboxIcon, SearchIcon, SendIcon, XIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { useShell } from "@/api/queries";
import { liveKeys } from "@/api/live";
import { topicKeys, useTopicFrame, useMessages } from "@/api/topics";
import type { MessagesResponse } from "@/api/types/MessagesResponse";
import type { RecordRow } from "@/api/types/RecordRow";
import type { Shown } from "@/api/types/Shown";
import type { TopicFrame } from "@/api/types/TopicFrame";
import { CopyButton } from "@/components/app/copy-button";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { EmptyState } from "@/components/app/empty-state";
import { MultiSelect } from "@/components/app/multi-select";
import { RefreshControl } from "@/components/app/refresh-control";
import { Spans } from "@/components/app/spans";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { number } from "@/lib/format";
import { bytes, clockMs } from "@/lib/kafka";
import { type MessagesSearch, messagesQuery, partitionParam, partitionsOf, recordAt } from "@/lib/topic-search";
import { useTailOwnsRefresh } from "@/lib/refresh";
import { cn } from "@/lib/utils";
import { PageError } from "../page-error";
import { WireDock } from "../live/wire-dock";
import { Explainers } from "./explain";
import { useTopicDialog } from "./frame";
import { RecordSheet } from "./record";
import { type Conn, SPEEDS, type Speed, WINDOW, prepend, rowId, useMessagesTail, useTailSpeed } from "./tail";
import { STANDING } from "./words";

type Patch = Partial<MessagesSearch>;

/** Sets search params on the page it is on (`/topics/x` or `/topics/x/messages`), keeping the rest. */
function useSetSearch() {
  const navigate = useNavigate();
  return useCallback(
    (patch: Patch, push = false) => void navigate({ to: ".", search: ((s: MessagesSearch) => ({ ...s, ...patch })) as never, replace: !push, resetScroll: false }),
    [navigate],
  );
}

function cellOf(s: Shown, what: "key" | "value") {
  if (s.kind === "null") return <span className="text-muted-foreground">null{what === "value" && " (deletion)"}</span>;
  if (s.kind === "empty") return <span className="text-muted-foreground">empty</span>;
  return s.preview;
}

/** Whether the window is wide (1280 px and more): under it the table drops Value size and narrows, so Value keeps room. */
const WIDE = "(min-width: 1280px)";
function useWide(): boolean {
  return useSyncExternalStore(
    (l) => {
      const m = window.matchMedia(WIDE);
      m.addEventListener("change", l);
      return () => m.removeEventListener("change", l);
    },
    () => window.matchMedia(WIDE).matches,
    () => true,
  );
}

function columns(open: (r: RecordRow) => void, wide: boolean): DataColumn<RecordRow>[] {
  const all: DataColumn<RecordRow>[] = [
    { accessorKey: "partition", header: "Partition", size: wide ? 96 : 86, meta: { mono: true } },
    {
      accessorKey: "offset",
      header: "Offset",
      size: wide ? 112 : 92,
      meta: { mono: true },
      cell: ({ row: { original: r } }) => (
        <button
          type="button"
          className="rounded-sm text-link tabular-nums hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
          onClick={(e) => {
            e.stopPropagation();
            open(r);
          }}
          data-testid="open-record"
        >
          {number(r.offset)}
        </button>
      ),
    },
    { id: "ts", accessorKey: "timestamp_ms", header: "Timestamp", size: wide ? 132 : 112, meta: { mono: true }, cell: (c) => <span className="text-muted-foreground">{clockMs(c.getValue<number>())}</span> },
    { id: "key", accessorFn: (r) => r.key.preview, header: "Key", size: wide ? 170 : 104, meta: { mono: true }, cell: ({ row: { original: r } }) => cellOf(r.key, "key") },
    {
      id: "value",
      accessorFn: (r) => r.value.preview,
      header: "Value",
      meta: { mono: true },
      cell: ({ row: { original: r } }) => <span className={r.value.kind === "binary" ? "text-muted-foreground" : "text-foreground"}>{cellOf(r.value, "value")}</span>,
    },
    { id: "size", accessorFn: (r) => r.value.bytes, header: "Value size", size: 108, meta: { numeric: true, mono: true }, cell: (c) => bytes(c.getValue<number>()) },
    {
      id: "standing",
      accessorFn: (r) => r.standing,
      // The narrow header is one short word: a header is never cut (the tooltip of each cell says what it means).
      header: wide ? "For read_committed" : "Status",
      size: wide ? 168 : 124,
      cell: ({ row: { original: r } }) => <StatusDot tone={STANDING[r.standing].tone}>{STANDING[r.standing].word}</StatusDot>,
    },
  ];
  return wide ? all : all.filter((c) => c.id !== "size");
}

/** The one box: digits are an offset, anything else a key (as the classic page reads it). */
function FindBox({ search, set }: { search: MessagesSearch; set: (p: Patch) => void }) {
  const current = search.key ?? (search.offset !== undefined ? String(search.offset) : "");
  const [text, setText] = useState(current);
  const [was, setWas] = useState(current);
  if (was !== current) {
    setWas(current);
    setText(current);
  }
  const apply = (t: string) => {
    const v = t.trim();
    if (!v) set({ key: undefined, offset: undefined, mode: undefined });
    else if (/^\d{1,15}$/.test(v)) set({ offset: Number(v), key: undefined, mode: undefined });
    else set({ key: v, offset: undefined, mode: undefined });
  };
  return (
    <form
      role="search"
      className="relative w-64 max-w-full"
      onSubmit={(e) => {
        e.preventDefault();
        apply(text);
      }}
    >
      <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
      <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="Key, or offset" aria-label="Find a key, or go to an offset" className="bg-card pr-9 pl-9 font-mono text-[13px]" spellCheck={false} autoComplete="off" maxLength={512} data-testid="find" />
      {text && (
        <button
          type="button"
          aria-label="Clear the key or offset"
          onClick={() => {
            setText("");
            apply("");
          }}
          className="absolute top-1/2 right-2 -translate-y-1/2 rounded-sm p-1 text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          <XIcon className="size-3.5" />
        </button>
      )}
    </form>
  );
}

const two = (n: number) => String(n).padStart(2, "0");
/** `2026-11-13 14:30`, UTC: the form the server reads. */
export function utcMinute(ms: number): string {
  const d = new Date(ms);
  return `${d.getUTCFullYear()}-${two(d.getUTCMonth() + 1)}-${two(d.getUTCDate())} ${two(d.getUTCHours())}:${two(d.getUTCMinutes())}`;
}

/** From time: a Popover with a field (UTC, as the console writes times), the quick choices, Apply and Clear. */
function TimePick({ search, set }: { search: MessagesSearch; set: (p: Patch) => void }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState(search.time ?? "");
  const presets = [
    { label: "5 min ago", ms: 5 * 60_000 },
    { label: "1 hour ago", ms: 3_600_000 },
    { label: "24 hours ago", ms: 86_400_000 },
  ];
  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) setText(search.time ?? "");
      }}
    >
      <PopoverTrigger asChild>
        <Button variant="outline" className={cn("max-w-64 bg-card font-normal", search.time && "border-primary/50 text-bright")} data-testid="time-trigger">
          <CalendarClockIcon />
          <span className="truncate">{search.time ? `From ${search.time}` : "From time"}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-80" align="start">
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            set({ time: text.trim() || undefined, mode: undefined });
            setOpen(false);
          }}
        >
          <Label htmlFor="from-time">From time, UTC</Label>
          <Input id="from-time" value={text} onChange={(e) => setText(e.target.value)} placeholder="2026-11-13 14:30" className="font-mono text-[13px]" autoComplete="off" spellCheck={false} maxLength={40} data-testid="time-input" autoFocus />
          <div className="flex flex-wrap gap-1.5">
            {presets.map((p) => (
              <Button key={p.label} type="button" variant="secondary" size="sm" className="h-7 px-2.5 text-[12.5px]" onClick={() => setText(utcMinute(Date.now() - p.ms))}>
                {p.label}
              </Button>
            ))}
          </div>
          <p className="text-[12px] text-muted-foreground">The first record at or after this time, in every partition read. A UTC time like 2026-11-09 22:13, or milliseconds since the epoch.</p>
          <div className="flex gap-2">
            <Button type="submit" size="sm" data-testid="time-apply">
              Apply
            </Button>
            {search.time && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => {
                  set({ time: undefined });
                  setOpen(false);
                }}
              >
                Clear
              </Button>
            )}
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}

function ConnDot({ conn }: { conn: Conn | null }) {
  const [now, setNow] = useState(() => Date.now());
  const down = conn?.state === "down";
  useEffect(() => {
    if (!down) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [down]);
  const box = "inline-flex min-w-[13rem] items-center";
  if (!conn) return <span className={box}><StatusDot tone="idle" data-testid="conn" data-state="snapshot">Snapshot</StatusDot></span>;
  if (conn.state === "live") return <span className={box}><StatusDot tone="ok" data-testid="conn" data-state="live">Live</StatusDot></span>;
  if (conn.state === "down") {
    const secs = Math.max(0, Math.ceil((conn.retryAt - now) / 1000));
    return (
      <span className={box}>
        <Tooltip>
          <TooltipTrigger asChild>
            <StatusDot tone="bad" tabIndex={0} data-testid="conn" data-state="down">
              Disconnected · Retry in <time className="inline-block w-[2.5ch] tabular-nums">{secs} s</time>
            </StatusDot>
          </TooltipTrigger>
          <TooltipContent>{conn.why}</TooltipContent>
        </Tooltip>
      </span>
    );
  }
  return <span className={box}><StatusDot tone="warn" data-testid="conn" data-state={conn.state}>{conn.state === "connecting" ? "Connecting…" : "Reconnecting…"}</StatusDot></span>;
}

/** Row two of the toolbar: always there, always one line: the filters that are on (each with a way off) or a quiet line;
 * at the right the Snapshot | Live tail switch, its speed and the connection. */
function FilterLine({ search, set, tailable, conn, refresh }: { search: MessagesSearch; set: (p: Patch) => void; tailable: boolean; conn: Conn | null; refresh?: React.ReactNode }) {
  const [speed, setSpeed] = useTailSpeed();
  const chips: { label: string; off: Patch; id: string }[] = [];
  const parts = partitionsOf(search.partition);
  for (const p of parts) chips.push({ id: `p${p}`, label: `partition ${p}`, off: { partition: partitionParam(parts.filter((x) => x !== p)) } });
  if (search.offset !== undefined) chips.push({ id: "offset", label: `from offset ${number(search.offset)}`, off: { offset: undefined } });
  if (search.time) chips.push({ id: "time", label: `from ${search.time}`, off: { time: undefined } });
  if (search.key) chips.push({ id: "key", label: `key ${search.key}`, off: { key: undefined } });
  if (search.limit) chips.push({ id: "limit", label: `${search.limit} records`, off: { limit: undefined } });
  const tail = search.mode === "tail" && tailable;
  return (
    <div className="flex min-h-9 flex-wrap items-center justify-between gap-x-4 gap-y-2" data-testid="filter-line">
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
        {chips.length === 0 ? (
          <span className="truncate text-[13px] text-muted-foreground" title="No filter: the newest records of every partition.">
            No filter: the newest records of every partition.
          </span>
        ) : (
          <>
            {chips.map((c) => (
              <Badge key={c.id} variant="secondary" className="h-7 gap-1 pr-1 pl-2.5 font-mono text-[12px]" data-testid={`chip-${c.id}`}>
                <span className="max-w-[16rem] truncate">{c.label}</span>
                <button type="button" aria-label={`Remove ${c.label}`} onClick={() => set(c.off)} className="rounded-full p-0.5 hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
                  <XIcon className="size-3" />
                </button>
              </Badge>
            ))}
            <Button variant="link" size="sm" className="h-7 px-1.5 text-[12.5px]" onClick={() => set({ partition: undefined, offset: undefined, time: undefined, key: undefined, limit: undefined })}>
              Clear all
            </Button>
          </>
        )}
      </div>
      <div className="flex flex-wrap items-center justify-end gap-3">
        <Tooltip>
          <TooltipTrigger asChild>
            <span tabIndex={tailable ? -1 : 0}>
              <ToggleGroup type="single" variant="outline" size="sm" value={tail ? "tail" : "snapshot"} onValueChange={(v) => v && set({ mode: v === "tail" ? "tail" : undefined, record: undefined })} aria-label="Snapshot or Live tail" className="bg-card" disabled={!tailable} data-testid="mode">
                <ToggleGroupItem value="snapshot" className="px-3">
                  Snapshot
                </ToggleGroupItem>
                <ToggleGroupItem value="tail" className="px-3">
                  Live tail
                </ToggleGroupItem>
              </ToggleGroup>
            </span>
          </TooltipTrigger>
          <TooltipContent>{tailable ? "Snapshot: the records as they were when the page loaded. Live tail: new records arrive at the top." : "A key, an offset or a time asks for a fixed read: only the newest records can be tailed."}</TooltipContent>
        </Tooltip>
        <Select value={speed} onValueChange={(v) => setSpeed(v as Speed)} disabled={!tail}>
          <SelectTrigger size="sm" className="w-[8.5rem] bg-card" aria-label="Tail speed" data-testid="speed">
            <SelectValue />
          </SelectTrigger>
          <SelectContent align="end">
            {SPEEDS.map((s) => (
              <SelectItem key={s.value} value={s.value} title={s.tip}>
                {s.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <ConnDot conn={tail ? conn : null} />
        {!tail && refresh}
      </div>
    </div>
  );
}

function Toolbar({ frame, search, set }: { frame: TopicFrame; search: MessagesSearch; set: (p: Patch) => void }) {
  const options = frame.head.parts.map((p) => ({ value: String(p.partition), label: `${frame.head.name}-${p.partition}`, hint: number(Math.max(0, p.end - p.start)) }));
  const chosen = partitionsOf(search.partition).map(String);
  return (
    <div className="flex flex-wrap items-center gap-3" data-testid="toolbar">
      <MultiSelect options={options} value={chosen} onChange={(v) => set({ partition: partitionParam(v) })} noun="partitions" aria-label="Partitions" chips={false} triggerClassName="w-48" />
      <FindBox search={search} set={set} />
      <TimePick search={search} set={set} />
      <div className="ml-auto flex items-center gap-2.5">
        <span className="text-[13px] text-muted-foreground" id="reading-l">
          Reading as
        </span>
        <ToggleGroup type="single" variant="outline" value={search.isolation ?? "read_uncommitted"} onValueChange={(v) => v && set({ isolation: v === "read_committed" ? "read_committed" : undefined })} aria-labelledby="reading-l" className="bg-card" data-testid="isolation">
          <ToggleGroupItem value="read_uncommitted" className="px-3 font-mono text-[12.5px]">
            read_uncommitted
          </ToggleGroupItem>
          <ToggleGroupItem value="read_committed" className="px-3 font-mono text-[12.5px]">
            read_committed
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
    </div>
  );
}

function IsolationNote({ r, frame }: { r: MessagesResponse; frame: TopicFrame }) {
  if (r.view.query.reading !== "committed") return null;
  const n = frame.held;
  return (
    <p className="mb-3 text-[13px] text-muted-foreground" role="status" data-testid="isolation-note">
      {n === 0
        ? "Reading as read_committed. Nothing is held back, so this is every record the table shows."
        : n === 1
          ? "Reading as read_committed. One record is held back (it is above the last stable offset) and is not listed; read_uncommitted shows it, marked Above LSO."
          : `Reading as read_committed. ${number(n)} records are held back (they are above the last stable offset) and are not listed; read_uncommitted shows them, each marked Above LSO.`}
    </p>
  );
}

/** The Refresh of a Messages snapshot, beside its Snapshot | Live tail switch (the switch owns how the page keeps up, so the
 * frame shows no page-wide RefreshControl here): the stamp and the button, which fetch the whole topic page again. */
function SnapshotRefresh({ topic }: { topic: string }) {
  const qc = useQueryClient();
  const frame = useTopicFrame(topic);
  const fetching = useIsFetching({ queryKey: topicKeys.topic(topic) }) > 0;
  const onRefresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: topicKeys.topic(topic) });
    void qc.invalidateQueries({ queryKey: liveKeys.topicWire(topic) });
  }, [qc, topic]);
  return <RefreshControl interval={false} updatedAt={frame.dataUpdatedAt} fetching={fetching} onRefresh={onRefresh} />;
}

/** The table, its pager and (in Live tail) the tail with its held pill. Keyed by the query and the mode, so a change of
 * either starts clean. */
function Records({ topic, r, frame, tail, tailable, search, set, open, children }: { topic: string; r: MessagesResponse; frame: TopicFrame; tail: boolean; tailable: boolean; search: MessagesSearch; set: (p: Patch) => void; open: (row: RecordRow) => void; children?: React.ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const hover = useRef(false);
  const [speed] = useTailSpeed();
  const pace = SPEEDS.find((s) => s.value === speed)?.pace ?? 3;
  const base = r.view.rows;
  const known = useMemo(() => new Set(base.map(rowId)), [base]);
  const isHeld = useCallback(() => {
    const el = box.current;
    if (!el) return false;
    const scroller = el.querySelector("[data-slot=data-table]");
    if (scroller && scroller.scrollTop > 2) return true;
    if (hover.current) return true;
    if (el.contains(document.activeElement)) return true;
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed && sel.anchorNode && el.contains(sel.anchorNode)) return true;
    return el.getBoundingClientRect().top < 0;
  }, []);
  const q = new URLSearchParams({ topic, pace: String(pace) });
  for (const p of r.view.query.partitions) q.append("partition", String(p));
  if (r.view.query.reading === "committed") q.set("isolation", "read_committed");
  const t = useMessagesTail({ path: tail ? `/api/v1/tail/messages?${q}` : null, cursor: r.cursor, isHeld, known });
  const rows = useMemo(() => (tail ? prepend(t.live, base, WINDOW + base.length) : base), [tail, t.live, base]);
  const wide = useWide();
  const cols = useMemo(() => columns(open, wide), [open, wide]);
  const filtered = r.view.query.partitions.length > 0 || r.view.query.offset !== null || r.view.query.time !== null || r.view.query.key !== null;
  const show = () => {
    box.current?.querySelector("[data-slot=data-table]")?.scrollTo({ top: 0 });
    t.show();
  };
  return (
    <>
    <FilterLine search={search} set={set} tailable={tailable} conn={tail ? t.conn : null} refresh={<SnapshotRefresh topic={topic} />} />
    {/* What is said about the read sits under the controls, so a sentence that comes or goes never moves them. */}
    {children && <div className="mt-3">{children}</div>}
    <div ref={box} className="relative mt-3" onPointerEnter={() => (hover.current = true)} onPointerLeave={() => (hover.current = false)} data-testid="records" data-tail={tail || undefined}>
      {tail && t.held.length > 0 && (
        <div className="pointer-events-none absolute inset-x-0 top-12 z-20 flex justify-center">
          <Button size="sm" className="pointer-events-auto rounded-full shadow-md" onClick={show} data-testid="held-pill">
            <ArrowUpIcon /> {number(t.held.length)} new · Show
          </Button>
        </div>
      )}
      <DataTable
        data={rows}
        columns={cols}
        rowId={rowId}
        aria-label="Records, newest first"
        maxHeight={680}
        onRowClick={open}
        rowClassName={(x) => (t.fresh.has(rowId(x)) ? "row-new" : undefined)}
        empty={filtered ? "No record matches these filters." : "No records to list."}
      />
      <div className="mt-3 flex min-h-9 items-center justify-end gap-3 text-[13px] text-muted-foreground tabular-nums" data-testid="pager">
        <span>
          Newest first · {number(rows.length)}
          {filtered ? (rows.length === 1 ? " record" : " records") : ` of ${number(frame.records)}`}
        </span>
        {!tail && r.older !== null && (
          <Button variant="outline" size="sm" className="bg-card" onClick={() => set({ limit: r.older ?? undefined })} data-testid="older">
            Older
          </Button>
        )}
      </div>
    </div>
    </>
  );
}

function EmptyTopic({ frame, listen }: { frame: TopicFrame; listen: string }) {
  const { open } = useTopicDialog();
  const cmd = `echo '{"order_id":"ord-88412","error":"payment declined"}' \\\n  | kcat -P -b ${listen} -t ${frame.head.name} -k ord-88412`;
  return (
    <div data-testid="messages-empty">
      <EmptyState
        icon={InboxIcon}
        title="No records yet"
        action={
          <Button onClick={() => open("produce")}>
            <SendIcon /> Produce a record
          </Button>
        }
      >
        Nothing has been written to this topic, so there is nothing to read, and no group or producer has touched it. The first record lands at offset 0 of partition 0.
      </EmptyState>
      <section className="mt-8">
        <h2 className="text-[17px] font-semibold text-bright">Or from your app</h2>
        <p className="mt-1 text-[13.5px] text-muted-foreground">The first record lands at offset 0 of partition 0.</p>
        <pre className="mt-3 overflow-x-auto rounded-xl border bg-well p-4 font-mono text-[12.5px] text-foreground">{cmd}</pre>
        <div className="mt-3">
          <CopyButton text={cmd} what="Command copied" />
        </div>
      </section>
    </div>
  );
}

function Offline({ frame }: { frame: TopicFrame }) {
  const set = useSetSearch();
  const h = frame.head;
  const down = h.parts.filter((p) => p.offline !== null);
  return (
    <div className="flex flex-col gap-6" data-testid="messages-offline">
      {down.map((p) => (
        <div key={p.partition} className="flex gap-3 rounded-xl border border-bad/30 bg-bad/5 p-4 text-[13.5px]" role="alert">
          <CircleAlertIcon className="mt-0.5 size-4.5 shrink-0 text-bad" aria-hidden />
          <p>
            <b className="font-semibold text-bright">
              {h.name}-{p.partition} is offline.
            </b>{" "}
            A flush of its active segment failed: <code className="font-mono">{p.offline}</code>. The broker does not retry it.
          </p>
        </div>
      ))}
      <div className="grid gap-4 lg:grid-cols-2">
        <div className="rounded-xl border bg-card p-5">
          <h3 className="text-[15px] font-semibold text-bright">What works and what does not</h3>
          <table className="mt-3 w-full text-[13px]">
            <thead className="text-left text-[12px] tracking-[0.04em] text-muted-foreground uppercase">
              <tr>
                <th className="h-9 font-medium">Partition</th>
                <th className="font-medium">State</th>
                <th className="font-medium">Writes</th>
                <th className="font-medium">Reads</th>
              </tr>
            </thead>
            <tbody>
              {h.parts.map((p) => (
                <tr key={p.partition} className="border-t">
                  <td className="h-10 font-mono">{p.partition}</td>
                  <td>{p.offline !== null ? <StatusDot tone="bad">Offline</StatusDot> : <StatusDot tone="ok">Online</StatusDot>}</td>
                  <td>{p.offline !== null ? <span className="font-mono text-refused">KAFKA_STORAGE_ERROR (56)</span> : "accepted"}</td>
                  <td>served</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="rounded-xl border bg-card p-5 text-[13.5px]">
          <h3 className="text-[15px] font-semibold text-bright">What restarting does</h3>
          <p className="mt-1 text-muted-foreground">An offline partition stays offline until restart. Restart the machine, not only doze.</p>
          <p className="mt-3">After a failed flush the kernel may mark the lost pages clean. A new doze process on the same machine can find them in the page cache, and its flush then succeeds without writing anything. Only starting the machine again clears that.</p>
          <p className="mt-3 text-muted-foreground">Records acknowledged with acks -1 were flushed before the failure. With acks 1 the last 500 ms before it may not have been.</p>
        </div>
      </div>
      <p className="text-[13px] text-muted-foreground">
        Its records can still be read:{" "}
        <button type="button" className="font-medium text-link hover:underline" onClick={() => set({ partition: down[0]?.partition ?? 0 }, true)}>
          open partition {down[0]?.partition ?? 0}
        </button>
        .
      </p>
    </div>
  );
}

export function MessagesPage() {
  const { topic } = useParams({ strict: false }) as { topic: string };
  const search = useSearch({ strict: false }) as MessagesSearch;
  const set = useSetSearch();
  const frame = useTopicFrame(topic);
  useTailOwnsRefresh();
  const qs = messagesQuery(search);
  const m = useMessages(topic, qs);
  const pushed = useRef(false);
  const open = useCallback(
    (row: RecordRow) => {
      pushed.current = true;
      set({ record: `${row.partition}:${row.offset}` }, true);
    },
    [set],
  );
  const close = () => {
    if (pushed.current) {
      pushed.current = false;
      window.history.back();
    } else set({ record: undefined });
  };
  const f = frame.data;
  const filtered = !!(search.partition !== undefined || search.offset !== undefined || search.time || search.key);
  const committed = search.isolation === "read_committed";
  const at = recordAt(search.record);
  const listen = useShell().data?.address ?? "";
  let body: React.ReactNode;
  if (!f) body = <Skeleton className="h-96 w-full rounded-xl" />;
  else if (f.empty && !filtered && !committed) body = <EmptyTopic frame={f} listen={listen} />;
  else if (f.offline.length > 0 && !filtered && !committed) body = <Offline frame={f} />;
  else {
    const r = m.data;
    const tailable = !search.key && search.offset === undefined && !search.time;
    body = (
      <div className="flex flex-col gap-3" data-testid="messages">
        <Toolbar frame={f} search={search} set={set} />
        {m.error && !r ? (
          <PageError error={m.error} onRetry={() => void m.refetch()} />
        ) : !r ? (
          <Skeleton className="h-96 w-full rounded-xl" />
        ) : (
          <div aria-busy={m.isFetching || undefined}>
            <Records key={`${qs}|${search.mode ?? ""}`} topic={topic} r={r} frame={f} tail={search.mode === "tail" && tailable} tailable={tailable} search={search} set={set} open={open}>
              {(r.view.notes.length > 0 || r.view.query.reading === "committed" || r.reach.length > 0) && (
                <>
                  {r.view.notes.map((n) => (
                    <p key={n} className="mb-2 text-[13px] text-warn" role="status">
                      {n}
                    </p>
                  ))}
                  <IsolationNote r={r} frame={f} />
                  {r.reach.length > 0 && (
                    <p className="max-w-[60rem] text-[13.5px] text-muted-foreground" data-testid="reach">
                      <Spans spans={r.reach} />
                    </p>
                  )}
                </>
              )}
            </Records>
            <p className="mt-5 text-[13px]">
              <StatusDot tone={f.offline.length ? "bad" : "ok"}>
                {f.head.parts.length - f.offline.length} of {f.head.parts.length} partitions online
              </StatusDot>
            </p>
            {/* Board topic-detail: the dock of the tape, filtered to this topic, under the records. */}
            <div className="mt-10">
              <WireDock topic={topic} />
            </div>
            <Explainers v={r.view} />
          </div>
        )}
      </div>
    );
  }
  return (
    <>
      {body}
      <RecordSheet topic={topic} at={at} onClose={close} />
    </>
  );
}

