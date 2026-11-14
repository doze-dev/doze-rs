// The frame of every page of one topic (slice R1a; R1b builds the Config tab and the write overlays on it): the name with its
// state words, the actions, the stat strip, the reserved transaction region, and the five tabs as routes. Every tab page
// renders under exactly this, so switching tabs changes nothing above the tab body.
import { useQueryClient } from "@tanstack/react-query";
import { Link, Outlet, useNavigate, useParams, useRouterState, useSearch } from "@tanstack/react-router";
import { CircleCheckIcon, ListIcon, PlusIcon, SearchIcon, SendIcon, Trash2Icon, TriangleAlertIcon } from "lucide-react";
import { useCallback } from "react";
import { ApiFailure } from "@/api/client";
import { topicKeys, useTopicFrame } from "@/api/topics";
import type { TopicFrame } from "@/api/types/TopicFrame";
import { EmptyState } from "@/components/app/empty-state";
import { RefreshControl } from "@/components/app/refresh-control";
import { type Stat, StatStrip } from "@/components/app/stat";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { bytes, clock, dateTime, listAnd, retention, stamp, timeoutText } from "@/lib/kafka";
import { cn } from "@/lib/utils";
import { PageError } from "../page-error";
import { TopicOverlays } from "./overlays";

export type TopicTab = "messages" | "log" | "config" | "consumers" | "producers";

/** Which part of a topic the address is: a tab, the partitions page (its own head, no tabs) or a transaction (Producers). */
export function placeOf(pathname: string, topic: string): TopicTab | "partitions" {
  const enc = `/topics/${encodeURIComponent(topic)}`;
  const at = pathname.indexOf(enc);
  const rest = at >= 0 ? pathname.slice(at + enc.length) : "";
  const first = rest.split("/")[1] ?? "";
  if (first === "partitions") return "partitions";
  if (first === "transactions" || first === "producers") return "producers";
  if (first === "log" || first === "config" || first === "consumers") return first;
  return "messages";
}

/** The overlay a topic page has open (`?dialog=`), and the ways to open and close it. Opening pushes a history entry, so Back
 * closes it. For R1b's dialogs. */
export function useTopicDialog() {
  const search = useSearch({ strict: false }) as { dialog?: string };
  const navigate = useNavigate();
  const open = useCallback(
    (dialog: string) => void navigate({ to: ".", search: (s: Record<string, unknown>) => ({ ...s, dialog }), resetScroll: false }),
    [navigate],
  );
  const close = useCallback(() => void navigate({ to: ".", search: (s: Record<string, unknown>) => ({ ...s, dialog: undefined }), replace: true, resetScroll: false }), [navigate]);
  return { dialog: search.dialog, open, close };
}

function stats(f: TopicFrame): Stat[] {
  const h = f.head;
  const last: Stat =
    f.last_write_ms !== null && !f.empty
      ? { label: "Last write", value: stamp(f.last_write_ms, h.now_ms), mono: true, ch: 11, unit: "UTC", tip: dateTime(f.last_write_ms) }
      : { label: "Last write", value: "never", mono: true, ch: 11 };
  return [
    { label: "Partitions", value: number(h.parts.length), ch: 4, unit: f.offline.length ? `${f.offline.length} offline` : "all online" },
    h.retention_ms !== null ? { label: "Retention", value: retention(h.retention_ms), unit: f.policy, mono: true, ch: 9 } : { label: "Retention", value: f.policy, mono: true, ch: 9 },
    { label: "Records", value: number(f.records), ch: 9 },
    { label: "Size on disk", value: bytes(f.bytes), ch: 9 },
    last,
  ];
}

/** The open-transaction region: the callout, or a calm line, in a block of one fixed height whichever it is and however wide
 * the window (CON-807). The callout is one line: its title, labelled figures (no sentence with live numbers in it, CON-805;
 * the less telling ones only where the region is wide enough, never wrapped), and the way to the transaction. */
const TXN_BOX = "@container flex h-12 items-center gap-3 overflow-hidden rounded-xl border px-4 text-[13.5px]";
function TxnFigure({ label, children, className, tip, testId }: { label: string; children: React.ReactNode; className?: string; tip?: string; testId?: string }) {
  return (
    <div className={cn("flex shrink-0 items-baseline gap-1.5 whitespace-nowrap", className)} title={tip} data-testid={testId}>
      <dt className="text-[11.5px] font-medium tracking-[0.06em] text-muted-foreground uppercase">{label}</dt>
      <dd className="font-medium text-bright tabular-nums">{children}</dd>
    </div>
  );
}
function TxnRegion({ f }: { f: TopicFrame }) {
  const h = f.head;
  const t = h.txns[0];
  if (!t) {
    return (
      <div className={cn(TXN_BOX, "border-dashed bg-card/60 text-muted-foreground")} data-testid="txn-region" data-state="calm">
        <CircleCheckIcon className="size-4 text-good" aria-hidden /> No open transactions.
      </div>
    );
  }
  const on = t.partitions.map(([p]) => `${h.name}-${p}`);
  const held = [...h.parts].filter((p) => p.end - p.last_stable > 0).sort((a, b) => b.end - b.last_stable - (a.end - a.last_stable))[0];
  return (
    <div className={cn(TXN_BOX, "border-warn/30 bg-warn/5")} role="status" data-testid="txn-region" data-state="open">
      <TriangleAlertIcon className="size-4.5 shrink-0 text-warn" aria-hidden />
      <b className="max-w-[16rem] min-w-0 shrink truncate font-semibold text-bright" title={t.id}>
        Transaction <span className="font-mono">{t.id}</span> open
      </b>
      <dl className="flex min-w-0 flex-1 items-baseline gap-x-5 overflow-hidden" aria-label="The open transaction">
        <TxnFigure label="Began">
          {clock(t.since_ms)} <span className="text-muted-foreground">UTC</span>
        </TxnFigure>
        {held && (
          <TxnFigure label="Held back" tip={`For read_committed: ${h.name}-${held.partition}, last stable offset ${number(held.last_stable)}, high watermark ${number(held.end)}`} testId="txn-held">
            <span className="inline-block min-w-[3ch] text-right">{number(held.end - held.last_stable)}</span>
          </TxnFigure>
        )}
        <TxnFigure label="Timeout" className="hidden @3xl:flex">
          {timeoutText(t.timeout_ms)}
        </TxnFigure>
        <TxnFigure label="Producer" className="hidden @5xl:flex">
          {t.producer_id} · epoch {t.producer_epoch}
        </TxnFigure>
        <TxnFigure label="On" className="hidden @6xl:flex" tip={listAnd(on)}>
          <span className="font-mono text-[12.5px]">{listAnd(on)}</span>
        </TxnFigure>
      </dl>
      <Link to="/topics/$topic/transactions/$txn" params={{ topic: h.name, txn: t.id }} className="shrink-0 font-medium whitespace-nowrap text-link hover:underline">
        Open the transaction
      </Link>
    </div>
  );
}

function Words({ f }: { f: TopicFrame }) {
  return (
    <span className="inline-flex flex-wrap items-center gap-2" data-testid="topic-words">
      {f.head.cleanup === "compact" ? (
        <StatusDot tone="ok">Compacted</StatusDot>
      ) : (
        <Badge variant="outline" className="font-mono text-[11.5px]">
          {f.policy}
        </Badge>
      )}
      {f.offline.length > 0 ? (
        <StatusDot tone="bad">
          Partition{f.offline.length === 1 ? "" : "s"} {listAnd(f.offline)} offline
        </StatusDot>
      ) : f.head.txns.length > 0 ? (
        <StatusDot tone="warn">Transaction open</StatusDot>
      ) : null}
    </span>
  );
}

function Actions({ f, place }: { f: TopicFrame; place: TopicTab | "partitions" }) {
  const { open } = useTopicDialog();
  const topic = f.head.name;
  if (place === "partitions") {
    return (
      <Button onClick={() => open("add-partitions")} data-testid="act-add-partitions">
        <PlusIcon /> Add partitions
      </Button>
    );
  }
  return (
    <>
      {f.acts !== "empty" && (
        <Button variant="outline" className="bg-card" asChild>
          <Link to="/topics/$topic/partitions" params={{ topic }} data-testid="act-partitions">
            <ListIcon /> Partitions
          </Link>
        </Button>
      )}
      <Button onClick={() => open("produce")} data-testid="act-produce">
        <SendIcon /> Produce
      </Button>
      {f.acts !== "offline" && (
        <Button variant="outline" className="border-destructive/40 bg-card text-destructive hover:bg-destructive/5 hover:text-destructive" onClick={() => open("delete")} data-testid="act-delete">
          <Trash2Icon /> Delete topic
        </Button>
      )}
    </>
  );
}

const TABS: { id: TopicTab; label: string; to: string }[] = [
  { id: "messages", label: "Messages", to: "/topics/$topic/messages" },
  { id: "log", label: "Log", to: "/topics/$topic/log" },
  { id: "config", label: "Config", to: "/topics/$topic/config" },
  { id: "consumers", label: "Consumers", to: "/topics/$topic/consumers" },
  { id: "producers", label: "Producers", to: "/topics/$topic/producers" },
];

/** The five tabs, the same links in the same order with the same counts on every page of the topic. */
function Tabs({ f, on }: { f: TopicFrame; on: TopicTab }) {
  const counts: Partial<Record<TopicTab, number>> = { consumers: f.head.tabs.consumers, producers: f.head.tabs.producers };
  return (
    <nav aria-label="Topic" className="mt-6 border-b" data-testid="topic-tabs">
      <ul className="-mb-px flex gap-1 overflow-x-auto">
        {TABS.map((t) => {
          const active = t.id === on;
          return (
            <li key={t.id}>
              <Link
                to={t.to}
                params={{ topic: f.head.name }}
                aria-current={active ? "page" : undefined}
                data-tab={t.id}
                className={cn(
                  "inline-flex h-11 items-center gap-2 border-b-2 border-transparent px-3 text-[14px] whitespace-nowrap text-muted-foreground transition-colors hover:text-bright focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none",
                  active && "border-primary font-medium text-bright",
                )}
              >
                {t.label}
                {/* A count of reserved width: a count that grows never moves the tabs after it. */}
                {counts[t.id] !== undefined && <span className="inline-block min-w-[6ch] rounded-full bg-muted px-1.5 text-center text-[11.5px] text-muted-foreground tabular-nums">{number(counts[t.id]!)}</span>}
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}

/** A name that is no topic. */
export function NoTopic({ name }: { name: string }) {
  return (
    <>
      <header className="mb-8">
        <nav aria-label="Breadcrumb" className="flex min-h-9 items-center text-[13px] text-muted-foreground">
          <Link to="/topics" className="hover:text-link">
            Topics
          </Link>
        </nav>
        <h1 className="mt-5 text-[34px] leading-[42px] font-light tracking-[-0.03em] text-bright">No such topic</h1>
      </header>
      <EmptyState
        icon={SearchIcon}
        title={`There is no topic called ${name}`}
        action={
          <Button asChild>
            <Link to="/topics">Back to Topics</Link>
          </Button>
        }
      >
        Or the console may not describe it.
      </EmptyState>
    </>
  );
}

/** The layout route of `/topics/$topic`: the frame, then the tab under it. */
export function TopicLayout() {
  const { topic } = useParams({ from: "/topics/$topic" });
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const place = placeOf(pathname, topic);
  const q = useTopicFrame(topic);
  const qc = useQueryClient();
  const refresh = useCallback(() => void qc.invalidateQueries({ queryKey: topicKeys.topic(topic) }), [qc, topic]);
  if (q.error) {
    return q.error instanceof ApiFailure && q.error.code === "UNKNOWN_TOPIC_OR_PARTITION" ? <NoTopic name={topic} /> : <PageError error={q.error} onRetry={() => void q.refetch()} />;
  }
  const f = q.data;
  return (
    <div data-testid="topic-frame" data-topic={topic}>
      <header className="mb-5" data-slot="page-header">
        <div className="flex min-h-9 flex-wrap items-center justify-between gap-x-6 gap-y-2">
          <nav aria-label="Breadcrumb" className="min-w-0">
            <ol className="flex flex-wrap items-center gap-2 text-[13px] text-muted-foreground">
              <li>
                <Link to="/topics" className="rounded-sm hover:text-link focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
                  Topics
                </Link>
              </li>
              {place === "partitions" && (
                <>
                  <li aria-hidden className="text-input">
                    ›
                  </li>
                  <li>
                    <Link to="/topics/$topic/messages" params={{ topic }} className="font-mono hover:text-link">
                      {topic}
                    </Link>
                  </li>
                </>
              )}
            </ol>
          </nav>
          {/* On Messages the Snapshot | Live tail switch owns the updates, and its own Refresh sits beside it. */}
          {place !== "messages" && <RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={refresh} />}
        </div>
        <div className="mt-3 flex flex-wrap items-start justify-between gap-x-8 gap-y-4">
          <div className="flex min-w-0 flex-1 basis-[28rem] flex-wrap items-center gap-x-4 gap-y-2">
            {/* One rule for titles: the name of a thing (a topic) in mono, a page's own title (Partitions) in the page face. */}
            {place === "partitions" ? (
              <h1 className="min-w-0 text-[34px] leading-[42px] font-light tracking-[-0.03em] text-bright">Partitions</h1>
            ) : (
              <h1 className="min-w-0 font-mono text-[27px] leading-[34px] font-medium tracking-[-0.01em] break-all text-bright">{topic}</h1>
            )}
            {f && place !== "partitions" && <Words f={f} />}
          </div>
          <div className="flex flex-none flex-wrap items-center gap-2.5 pt-0.5">{f ? <Actions f={f} place={place} /> : <Skeleton className="h-9 w-72" />}</div>
        </div>
      </header>
      {/* Dense: the topic's records are the point of its pages, so its head takes as little height as it can. */}
      {f ? <StatStrip stats={stats(f)} dense /> : <Skeleton className="h-[86px] w-full rounded-xl" />}
      {place !== "partitions" && <div className="mt-3">{f ? <TxnRegion f={f} /> : <Skeleton className="h-12 w-full rounded-xl" />}</div>}
      {place !== "partitions" && f && <Tabs f={f} on={place} />}
      {place !== "partitions" && !f && <Skeleton className="mt-6 h-11 w-full" />}
      <div className="mt-5" data-testid="topic-body">
        {f ? <Outlet /> : <Skeleton className="h-96 w-full rounded-xl" />}
      </div>
      {f && <TopicOverlays frame={f} />}
    </div>
  );
}
