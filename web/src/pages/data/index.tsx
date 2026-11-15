// Data (docs/port/data-control.md): what is using the disk and why, and the ways to give it back. The page is a snapshot that
// follows the RefreshControl; each look also measures how fast each topic grows (nothing measures it while no page looks).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { EraserIcon, RotateCcwIcon, TerminalIcon } from "lucide-react";
import { useData } from "@/api/data";
import type { DataView } from "@/api/types/DataView";
import { DiskAlarmBanner } from "@/components/app/disk-alarm";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { type Stat, StatStrip } from "@/components/app/stat";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { bytes } from "@/lib/kafka";
import type { DataSearch } from "@/lib/search";
import { PageError } from "../page-error";
import { SectionHead, useRoutedOverlay } from "../r4/bits";
import { DataDialogs } from "./dialogs";
import { growth, TopicsSection } from "./topics";
import { UsageHero } from "./usage";

function stats(v: DataView): Stat[] {
  const largest = v.topics[0];
  const unbounded = v.topics.filter((t) => t.unbounded);
  const fastest = v.topics.filter((t) => (t.growth_per_hour ?? 0) > 0).sort((a, b) => (b.growth_per_hour ?? 0) - (a.growth_per_hour ?? 0))[0];
  return [
    largest ? { label: "Largest topic", value: largest.name, unit: bytes(largest.bytes), mono: true, ch: 16 } : { label: "Largest topic", value: "none", unit: "no topics", ch: 16 },
    { label: "Keep everything", value: String(unbounded.length), unit: unbounded.length ? `${bytes(unbounded.reduce((a, t) => a + t.bytes, 0))} with no limit` : "every topic has a limit", ch: 4 },
    fastest ? { label: "Growing fastest", value: fastest.name, unit: growth(fastest.growth_per_hour), mono: true, ch: 16 } : { label: "Growing fastest", value: "–", unit: v.growth_since_ms === null ? "measured from the next look" : "nothing grew lately", ch: 16 },
  ];
}

/** The two that reach every topic, with what each takes and keeps, and the same from the command line. */
function GiveBack({ v, onOpen }: { v: DataView; onOpen: (d: "empty-all" | "erase", from: HTMLElement) => void }) {
  const topicsBytes = v.kinds.find((k) => k.kind === "topics")?.bytes ?? 0;
  return (
    <section aria-labelledby="give-back" data-testid="give-back">
      <SectionHead id="give-back" title="Give space back" sub="One topic is emptied from its row above. These reach every topic at once: each says exactly what goes and what stays before it does anything, and asks you to type what you mean." />
      <div className="grid gap-4 xl:grid-cols-2">
        <div className="flex flex-col gap-4 rounded-xl border bg-card p-6">
          <div className="flex items-start gap-3">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-warn/10 text-warn">
              <EraserIcon aria-hidden className="size-[18px]" />
            </span>
            <div className="min-w-0 flex-1">
              <h3 className="text-[16px] font-semibold text-bright">Empty all topics</h3>
              <p className="mt-1 text-[13.5px] leading-[21px] text-muted-foreground">Every record of every topic goes, compacted ones too, and the disk is freed now. Topics, partitions, configs and committed offsets stay; each partition carries on from its end.</p>
            </div>
          </div>
          <div className="mt-auto flex flex-wrap items-center justify-between gap-3 border-t pt-4">
            <span className="text-[13px] text-muted-foreground">
              Frees about <b className="font-mono font-medium text-foreground">{bytes(topicsBytes)}</b>
            </span>
            <Button variant="outline" className="border-destructive/40 bg-card text-destructive hover:bg-destructive/10 hover:text-destructive" onClick={(e) => onOpen("empty-all", e.currentTarget)} disabled={v.topics.length === 0} data-testid="open-empty-all">
              <EraserIcon /> Empty all topics…
            </Button>
          </div>
        </div>
        <div className="flex flex-col gap-4 rounded-xl border border-destructive/25 bg-destructive/[0.03] p-6">
          <div className="flex items-start gap-3">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-destructive/10 text-destructive">
              <RotateCcwIcon aria-hidden className="size-[18px]" />
            </span>
            <div className="min-w-0 flex-1">
              <h3 className="text-[16px] font-semibold text-bright">Erase everything</h3>
              <p className="mt-1 text-[13.5px] leading-[21px] text-muted-foreground">Start fresh: every topic, group and committed offset, finished transaction and schema goes. ACLs, users and the broker's settings stay, so whoever could connect still can.</p>
            </div>
          </div>
          <div className="mt-auto flex flex-wrap items-center justify-between gap-3 border-t pt-4">
            <span className="text-[13px] text-muted-foreground">
              Frees about <b className="font-mono font-medium text-foreground">{bytes(topicsBytes + v.kinds.filter((k) => k.kind === "strays").reduce((a, k) => a + k.bytes, 0))}</b>
            </span>
            <Button variant="destructive" onClick={(e) => onOpen("erase", e.currentTarget)} data-testid="open-erase">
              Erase everything…
            </Button>
          </div>
        </div>
      </div>
      <p className="mt-4 flex max-w-[60rem] items-start gap-2 text-[12.5px] leading-[19px] text-muted-foreground">
        <TerminalIcon aria-hidden className="mt-0.5 size-3.5 shrink-0" />
        <span>
          The same from a terminal, with the broker stopped: <code className="font-mono text-foreground">doze-kafka data</code> says what is here, <code className="font-mono text-foreground">data empty --topic NAME</code>, <code className="font-mono text-foreground">data empty --all-topics --yes</code> and <code className="font-mono text-foreground">data erase --everything --yes</code> give it back.
        </span>
      </p>
    </section>
  );
}

function DataSkeleton() {
  return (
    <div className="flex flex-col gap-8" aria-busy>
      <Skeleton className="h-[300px] rounded-xl" />
      <Skeleton className="h-[98px] rounded-xl" />
      <Skeleton className="h-96 rounded-xl" />
    </div>
  );
}

export function DataPage() {
  const q = useData();
  const search = useSearch({ from: "/data" });
  const navigate = useNavigate({ from: "/data" });
  const overlay = useRoutedOverlay((patch, replace) => void navigate({ search: (s: DataSearch) => ({ ...s, ...patch }), replace, resetScroll: false }));
  const r = q.data;
  const v = r?.view;
  const toRow = (topic: string | null) => {
    if (!topic) return;
    document.getElementById(`r-${topic}`)?.scrollIntoView({ block: "center", behavior: "smooth" });
  };
  return (
    <>
      <PageHeader title="Data" sub="What this broker keeps on disk, what each part of it is, and why it is that size. Give the space back a topic at a time, or all at once." refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} />} />
      {q.error && !r ? (
        <PageError error={q.error} onRetry={() => void q.refetch()} />
      ) : !v ? (
        <DataSkeleton />
      ) : (
        <div className="flex flex-col gap-10" data-testid="data-page">
          <div className="flex flex-col gap-4">
            {v.alarm && (
              <DiskAlarmBanner
                alarm={v.alarm}
                action={
                  v.alarm.topic ? (
                    <Button variant="outline" className="bg-card" onClick={() => toRow(v.alarm?.topic ?? null)} data-testid="alarm-show">
                      Show {v.alarm.topic}
                    </Button>
                  ) : undefined
                }
              />
            )}
            <UsageHero v={v} />
            <StatStrip stats={stats(v)} dense />
          </div>
          <TopicsSection v={v} onEmpty={(topic, from) => overlay.open({ dialog: "empty", topic }, from)} />
          <GiveBack v={v} onOpen={(d, from) => overlay.open({ dialog: d, topic: undefined }, from)} />
        </div>
      )}
      <DataDialogs dialog={search.dialog} topic={search.topic} data={r} onClose={() => overlay.close({ dialog: undefined, topic: undefined })} />
    </>
  );
}
