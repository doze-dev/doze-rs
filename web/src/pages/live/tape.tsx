// The table of requests in the four layouts the boards draw (pages/live/tape.rs): Live, a topic's dock, a group's wire, one
// client's first requests. A DataTable: fixed columns the page declares (chosen by the width the table has, never by its rows),
// one line a row with the whole text on hover, the request id as the row's key.
import { Link } from "@tanstack/react-router";
import { FlaskConicalIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { WireRow } from "@/api/types/WireRow";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { number } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { TapeRow } from "./tail";
import { classTone, classWord, clockMs, dateTime, labDid, showsCode, took } from "./words";

export type Layout = "live" | "topic" | "group" | "first";

/** A click on a client, an API or a topic: the Live page filtered by it. */
export type FilterBy = (field: "client" | "api" | "topic", value: string) => void;

const TONE: Record<string, string> = { ok: "text-good", warn: "text-warn", bad: "text-bad", refused: "text-refused", idle: "text-muted-foreground", info: "text-cool" };

/** Widths in px for a table at least 1000 px wide, and for a narrower one (1024 px windows). The target takes the rest. */
const WIDE = { time: 128, client: 168, api: 196, result: 300, took: 84 };
const NARROW = { time: 122, client: 104, api: 128, result: 134, took: 84 };

function FilterButton({ onClick, title, className, children }: { onClick?: () => void; title: string; className?: string; children: React.ReactNode }) {
  if (!onClick) return <span className={className}>{children}</span>;
  return (
    <button
      type="button"
      title={title}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className={cn("max-w-full truncate rounded-sm text-left hover:text-primary hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", className)}
    >
      {children}
    </button>
  );
}

function Tag({ children, title, className }: { children: React.ReactNode; title?: string; className?: string }) {
  return (
    <span title={title} className={cn("ml-1.5 inline-flex h-5 shrink-0 items-center rounded-full border bg-well px-1.5 font-mono text-[11px] leading-none text-muted-foreground", className)}>
      {children}
    </span>
  );
}

export function TimeCell({ r }: { r: TapeRow }) {
  return (
    <Link to="/live" search={{ request: r.id }} onClick={(e) => e.stopPropagation()} className="rounded-sm font-mono text-[12.5px] text-primary hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" title={dateTime(r.at_ms)} data-testid="row-time">
      {r.fresh && <span data-fresh hidden />}
      {clockMs(r.at_ms)}
    </Link>
  );
}

export function ResultCell({ r }: { r: WireRow }) {
  const tone = TONE[classTone(r.class)];
  return (
    <span className="flex min-w-0 items-center gap-1.5" title={`${r.code_name} ${r.detail}`.trim()}>
      <span aria-hidden className={cn("size-2 shrink-0 rounded-full bg-current", tone)} />
      <b className={cn("shrink-0 font-semibold", tone)} data-testid="row-result">
        {classWord(r.class, r.code)}
      </b>
      {showsCode(r) ? (
        <span className="min-w-0 truncate">
          <span className="font-mono text-[12px] text-foreground">
            {r.code_name} ({r.code})
          </span>
          {r.detail && <span className="text-muted-foreground"> {r.detail}</span>}
        </span>
      ) : (
        r.detail && <span className="min-w-0 truncate text-muted-foreground">{r.detail}</span>
      )}
      {r.lab && (
        <Tag title={`Lab rule ${r.lab.rule}, ${r.lab.name}: ${labDid(r.lab.what)}`} className="border-primary/30 text-primary">
          <FlaskConicalIcon className="mr-0.5 size-3" aria-hidden />
          lab
        </Tag>
      )}
    </span>
  );
}

/** The columns of a layout at a width. */
export function tapeColumns(layout: Layout, wide: boolean, filter?: FilterBy): DataColumn<TapeRow>[] {
  const w = wide ? WIDE : NARROW;
  const target = layout === "topic" ? "Topic and partition" : layout === "group" ? "What it touched" : "Target";
  const api = layout === "group" ? "Request" : "API";
  const f = layout === "live" ? filter : undefined;
  const cols: DataColumn<TapeRow>[] = [
    { id: "time", header: "Time", size: w.time, enableSorting: false, cell: (c) => <TimeCell r={c.row.original} /> },
    {
      id: "client",
      header: "Client",
      size: w.client,
      enableSorting: false,
      cell: (c) => {
        const r = c.row.original;
        return (
          <span className="flex min-w-0 items-center">
            <FilterButton onClick={f && (() => f("client", r.client))} title={`Show only ${r.client}`} className="font-mono text-[12.5px] text-foreground">
              {r.client || "(no client.id)"}
            </FilterButton>
            {r.tag && <Tag>{r.tag}</Tag>}
          </span>
        );
      },
    },
    {
      id: "api",
      header: api,
      size: w.api,
      enableSorting: false,
      cell: (c) => {
        const r = c.row.original;
        return (
          <span className="flex min-w-0 items-center">
            <FilterButton onClick={f && (() => f("api", r.api))} title={`Show only ${r.api}`} className="font-mono text-[12.5px] font-semibold text-bright">
              {r.api}
            </FilterButton>
            <span className="ml-1 shrink-0 font-mono text-[12px] text-muted-foreground">v{r.version}</span>
            {r.transactional && <Tag>txn</Tag>}
            {r.count > 1 && (
              <Tag title={`${number(r.count)} requests folded into this row`} className="text-foreground">
                × {number(r.count)}
              </Tag>
            )}
          </span>
        );
      },
    },
    {
      id: "target",
      header: target,
      enableSorting: false,
      cell: (c) => {
        const r = c.row.original;
        const topic = r.topics[0];
        const sub = [r.target_sub, r.since_ms ? `since ${clockMs(r.since_ms).slice(0, 8)}` : null].filter(Boolean).join(" · ");
        return (
          <span className="flex min-w-0 items-baseline gap-2" title={sub ? `${r.target} · ${sub}` : r.target}>
            <FilterButton onClick={f && topic ? () => f("topic", topic) : undefined} title={`Show only ${topic ?? r.target}`} className="max-w-full min-w-0 shrink-0 truncate font-mono text-[12.5px] text-foreground">
              {r.target || "-"}
            </FilterButton>
            {sub && <span className="min-w-0 truncate font-mono text-[11.5px] text-muted-foreground">{sub}</span>}
          </span>
        );
      },
    },
    { id: "result", header: "Result", size: w.result, enableSorting: false, cell: (c) => <ResultCell r={c.row.original} /> },
    {
      id: "took",
      header: "Took",
      size: w.took,
      enableSorting: false,
      meta: { numeric: true, mono: true },
      cell: (c) => {
        const r = c.row.original;
        return <span title={r.took_sub ?? undefined}>{took(r.took_us)}</span>;
      },
    },
  ];
  return layout === "first" ? cols.filter((c) => c.id !== "client") : cols;
}

/** Whether an element is at least 1000 px wide, followed as it resizes (the columns change with the width, never the rows). */
export function useWide(el: HTMLElement | null): boolean {
  // A first guess from the window (the sidebar and the margins take about 290 px), so the first paint is already right.
  const [wide, setWide] = useState(() => (globalThis.innerWidth ?? 1440) - 290 >= 1000);
  useEffect(() => {
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setWide((e?.contentRect.width ?? 1200) >= 1000));
    ro.observe(el);
    return () => ro.disconnect();
  }, [el]);
  return wide;
}

/** A table of requests: a DataTable in one of the layouts. */
export function Tape({ rows, layout, filter, empty, maxHeight, label, onOpen, testId }: { rows: TapeRow[]; layout: Layout; filter?: FilterBy; empty?: React.ReactNode; maxHeight?: number; label: string; onOpen?: (r: TapeRow) => void; testId?: string }) {
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  const wide = useWide(el);
  // The columns are made once per layout, width and filter: a cell renderer of a new identity would remount every cell
  // (TanStack renders a function as a component), and the tail's rule is that an existing row is never rewritten.
  const columns = useMemo(() => tapeColumns(layout, wide, filter), [layout, wide, filter]);
  return (
    <div ref={setEl} className="min-w-0" data-testid={testId}>
      <DataTable data={rows} columns={columns} rowId={(r) => String(r.id)} empty={empty} maxHeight={maxHeight} aria-label={label} onRowClick={onOpen} className="tape" />
    </div>
  );
}
