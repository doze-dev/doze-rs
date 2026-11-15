// The topics on disk, largest first: each one's size and share, what it keeps and why (the server's sentence), how fast it
// grew lately, and the two ways to give its space back: Empty here, Delete on its page.
import { Link } from "@tanstack/react-router";
import { EraserIcon, InfoIcon, Trash2Icon } from "lucide-react";
import { useMemo } from "react";
import type { DataTopicRow } from "@/api/types/DataTopicRow";
import type { DataView } from "@/api/types/DataView";
import { type DataColumn, DataTable } from "@/components/app/data-table";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { bytes, dateTime, stamp } from "@/lib/kafka";
import { hhmmUtc } from "@/lib/format";
import { SectionHead, narrowed, useMinWidth, useStable } from "../r4/bits";
import { share } from "./usage";

/** `+21.4 MiB/h`, `+293 MiB/h`, `−3.0 KiB/h`, `steady`: a tenth only below ten of the unit, so it fits its column. */
export function growth(perHour: number | null): string {
  if (perHour === null) return "–";
  if (perHour === 0) return "steady";
  const size = bytes(Math.abs(perHour)).replace(/^(\d{2,})\.\d /, "$1 ");
  return `${perHour > 0 ? "+" : "−"}${size}/h`;
}

function columns(now: number, onEmpty: (topic: string, from: HTMLElement) => void): DataColumn<DataTopicRow>[] {
  return [
    {
      accessorKey: "name",
      header: "Topic",
      cell: ({ row: { original: t } }) => (
        <div className="flex min-w-0 flex-col gap-1 py-1.5">
          <div className="flex min-w-0 items-baseline gap-2">
            <Link to="/topics/$topic/messages" params={{ topic: t.name }} className="min-w-0 shrink truncate font-mono text-[13.5px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="data-topic">
              {t.name}
            </Link>
            <span className="shrink-0 text-[12px] text-muted-foreground tabular-nums">
              {t.partitions === 1 ? "1 partition" : `${t.partitions} partitions`}
            </span>
            {t.txn_open && (
              <StatusDot tone="warn" className="min-w-0 self-center overflow-hidden text-[12px]">
                <span className="truncate">Transaction open</span>
              </StatusDot>
            )}
          </div>
          {/* Why it is its size: the point of the page, so it wraps to a second line rather than being cut. */}
          <span className="line-clamp-2 text-[12.5px] leading-[17px] whitespace-normal text-muted-foreground" title={t.why} data-testid="why">
            {t.why}
          </span>
        </div>
      ),
    },
    {
      accessorKey: "bytes",
      header: "Size",
      size: 156,
      meta: { numeric: true },
      cell: ({ row: { original: t } }) => (
        <div className="flex flex-col items-end gap-2">
          <span className="font-mono text-[13px] text-bright">{bytes(t.bytes)}</span>
          <span className="flex items-center gap-2" title={`${share(t.share_permille)} of the data directory`}>
            <span aria-hidden className="block h-1 w-[72px] overflow-hidden rounded-full bg-track">
              <span className="block h-full rounded-full bg-cool" style={{ width: `${Math.max(t.bytes > 0 ? 4 : 0, t.share_permille / 10)}%` }} />
            </span>
            <span className="w-10 text-right font-mono text-[11.5px] text-muted-foreground tabular-nums" data-testid="share">
              {share(t.share_permille)}
            </span>
          </span>
        </div>
      ),
    },
    {
      id: "oldest",
      accessorFn: (t) => t.oldest_ms ?? Number.MAX_SAFE_INTEGER,
      header: "Oldest",
      size: 120,
      meta: { numeric: true, mono: true },
      cell: ({ row: { original: t } }) => (t.oldest_ms === null ? <span className="text-muted-foreground">–</span> : <span title={dateTime(t.oldest_ms)}>{stamp(t.oldest_ms, now)}</span>),
    },
    {
      id: "retention",
      accessorFn: (t) => (t.unbounded ? "" : t.retention),
      header: "Retention",
      size: 176,
      cell: ({ row: { original: t } }) =>
        t.unbounded ? (
          <Badge variant="outline" className="border-warn/50 bg-warn/10 font-medium text-warn" data-testid="unbounded">
            Keeps everything
          </Badge>
        ) : (
          <span className="flex min-w-0 items-baseline gap-1.5">
            <span className="truncate text-foreground">{t.retention}</span>
            {!t.own_retention && t.cleanup !== "compact" && (
              <span className="shrink-0 text-[12px] text-muted-foreground" title="The broker's default: the topic sets no retention of its own">
                default
              </span>
            )}
          </span>
        ),
    },
    {
      id: "growth",
      accessorFn: (t) => t.growth_per_hour ?? Number.MIN_SAFE_INTEGER,
      header: "Growth",
      size: 112,
      meta: { numeric: true, mono: true },
      cell: ({ row: { original: t } }) => <span className={t.growth_per_hour && t.growth_per_hour > 0 ? "text-foreground" : "text-muted-foreground"}>{growth(t.growth_per_hour)}</span>,
    },
    {
      id: "actions",
      header: () => <span className="sr-only">Give space back</span>,
      size: 124,
      enableSorting: false,
      cell: ({ row: { original: t } }) => (
        <div className="flex items-center justify-end gap-1">
          <Button variant="outline" size="sm" className="h-7 bg-card" onClick={(e) => onEmpty(t.name, e.currentTarget)} aria-label={`Empty ${t.name}`} data-testid={`empty-${t.name}`}>
            <EraserIcon /> Empty
          </Button>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="sm" className="h-7 w-7 px-0 text-muted-foreground hover:text-destructive" asChild>
                <Link to="/topics/$topic" params={{ topic: t.name }} search={{ dialog: "delete" }} aria-label={`Delete ${t.name}`} data-testid={`delete-${t.name}`}>
                  <Trash2Icon />
                </Link>
              </Button>
            </TooltipTrigger>
            <TooltipContent>Delete the topic, on its page</TooltipContent>
          </Tooltip>
        </div>
      ),
    },
  ];
}

export function TopicsSection({ v, onEmpty }: { v: DataView; onEmpty: (topic: string, from: HTMLElement) => void }) {
  const empty = useStable(onEmpty);
  const wide = useMinWidth(1280);
  const cols = useMemo(() => columns(v.now_ms, empty), [v.now_ms, empty]);
  return (
    <section aria-labelledby="data-topics">
      <SectionHead id="data-topics" title="Topics, largest first" sub="What each topic holds, and why. Empty removes every record and frees the disk now; the topic, its configs and its readers' commits stay. Delete removes the topic itself." />
      <DataTable data={v.topics} columns={narrowed(cols, wide, ["oldest"], "name")} rowId={(t) => t.name} aria-label="Topics by size on disk" rowHeight={68} maxHeight={640} empty="No topics: the data directory holds the metadata store and nothing else." />
      <div className="mt-3 flex max-w-[60rem] flex-col gap-1.5 text-[12.5px] leading-[19px] text-muted-foreground" data-testid="data-notes">
        <p className="flex items-start gap-2">
          <InfoIcon aria-hidden className="mt-0.5 size-3.5 shrink-0" />
          <span>{v.defaults}</span>
        </p>
        <p className="pl-[22px]">{v.growth_since_ms !== null ? `Growth is measured from this page's own looks, since ${hhmmUtc(v.growth_since_ms)} UTC. Nothing measures it while no page is open.` : "Growth shows once this page has looked twice, a minute apart. Nothing measures it while no page is open."}</p>
        {v.hidden_bytes > 0 && <p className="pl-[22px]">{bytes(v.hidden_bytes)} belong to topics the console may not describe.</p>}
        {v.strays.length > 0 && (
          <p className="pl-[22px]">
            Belonging to no topic: {v.strays.map(([name, b]) => `${name} (${bytes(b)})`).join(", ")}. Erase everything removes them.
          </p>
        )}
      </div>
    </section>
  );
}
