// A stream's page (kinesis__stream-detail and its tabs): the header with its status, the sentence, the strip from producers to consumers, the Summary
// card and the tabs Configuration, Data Viewer and Data stream sharing. Manage tags and the Visual policy editor are pages of their own, chosen by `?view=`.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { lazy, Suspense, useMemo, useState } from "react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, FlowStrip, RecentCalls, ResourcePage, ResourcePageSkeleton, useFeeders, useTags } from "../../../kit";
import { callOp } from "../deep/api";
import { timeMs } from "../model";
import { on, useConsumerNames } from "./api";
import { ConfigTab } from "./config";
import { DeleteStream } from "./delete";
import { type KRecord, arrivalMs, hoursWords, modeWords, count, statusWords, streamOf, streamSentence } from "./model";
import { SharingTab } from "./sharing";
import { ViewerTab } from "./viewer";

const Sharing = lazy(() => import("./sharing").then((m) => ({ default: ({ d }: { d: ResourceDetail }) => <m.SharingCreate s={streamOf(d.name, d.fields)} /> })));
const Manage = lazy(() => import("./tags-page").then((m) => ({ default: ({ d }: { d: ResourceDetail }) => <m.ManageTags name={d.name} /> })));

export function KinesisDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => { void refetch(); void qc.invalidateQueries({ queryKey: ["resources", "deep", "kinesis", "streams", name] }); void qc.invalidateQueries({ queryKey: ["resources", "tags", service, kind, name] }); };
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={refresh} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <StreamView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

/** The oldest record the Data Viewer can reach: the first shard read from its trim horizon, one record. */
function useOldest(name: string, shards: number) {
  return useQuery({
    queryKey: ["resources", "deep", "kinesis", "streams", name, "oldest", shards],
    queryFn: async () => {
      const list = await callOp<{ Shards?: { ShardId: string }[] }>(on(name), "ListShards", {});
      for (const sh of list.Shards ?? []) {
        const it = await callOp<{ ShardIterator: string }>(on(name), "GetShardIterator", { ShardId: sh.ShardId, ShardIteratorType: "TRIM_HORIZON" });
        const got = await callOp<{ Records?: KRecord[] }>(on(name), "GetRecords", { ShardIterator: it.ShardIterator, Limit: 1 });
        const at = got.Records?.[0] ? arrivalMs(got.Records[0]) : null;
        if (at !== null) return { shard: sh.ShardId, at };
      }
      return null;
    },
    retry: false,
  });
}

function Summary({ s }: { s: ReturnType<typeof streamOf> }) {
  const row = (l: string, v: React.ReactNode) => (
    <div key={l} className="contents"><dt className="border-b py-3.5 pr-4 text-[13.5px] text-muted-foreground">{l}</dt><dd className="min-w-0 border-b py-3.5 text-[13.5px] break-words text-bright" data-testid={`field-${l}`}>{v}</dd></div>
  );
  const col = "grid grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] content-start [&>div:last-child>*]:border-b-0";
  return (
    <section className="mb-10 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Summary" data-testid="details">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Summary</h2>
      <p className="mt-1 text-[13.5px] text-muted-foreground">The fields DescribeStreamSummary returns for this stream.</p>
      <dl className={`mt-3 ${col}`}>{row("Stream ARN", <span className="flex items-center gap-2 font-mono text-[13px]">{s.arn}<CopyButton text={s.arn} what="ARN copied" iconOnly variant="ghost" label="Copy ARN" /></span>)}</dl>
      <div className="grid lg:grid-cols-2 lg:gap-x-12">
        <dl className={col}>
          {row("Status", <StatusDot tone={s.status === "ACTIVE" ? "ok" : "info"}>{statusWords(s.status)}</StatusDot>)}
          {row("Capacity mode", modeWords(s.mode))}
          {row("Open shards", s.shards)}
          {row("Data retention period", hoursWords(s.retention))}
        </dl>
        <dl className={col}>
          {row("Created", s.created && timeMs(s.created) ? dateTimeUtc(timeMs(s.created) ?? 0) : "–")}
          {row("Server-side encryption", s.encryption === "KMS" ? "Enabled" : "Disabled")}
          {row("Registered consumers", s.consumers)}
        </dl>
      </div>
    </section>
  );
}

function StreamView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useSearch({ strict: false }) as { view?: string; done?: string; tab?: string };
  const navigate = useNavigate();
  const s = useMemo(() => streamOf(d.name, d.fields), [d]);
  const target = { service: d.service, kind: d.kind, name: d.name };
  const tags = useTags(target);
  const consumers = useConsumerNames(s.consumers > 0 ? { [d.name]: s.arn } : {})[d.name] ?? [];
  const feeders = useFeeders(d.calls, ["PutRecord", "PutRecords"]);
  const oldest = useOldest(d.name, s.shards);
  const [deleting, setDeleting] = useState(false);
  const metrics = useMemo<string[]>(() => { try { const v = JSON.parse(d.fields.find((f) => f.label === "Shard-level metrics")?.value ?? "[]") as unknown; return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []; } catch { return []; } }, [d]);
  const go = (patch: { view?: string; tab?: string; done?: string }) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: patch });
  if (search.view === "sharing-create" || search.view === "tags") {
    const Page = search.view === "tags" ? Manage : Sharing;
    return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}><Page d={d} /></Suspense>;
  }
  const tab = search.tab === "viewer" || search.tab === "sharing" ? search.tab : "configuration";
  const created = s.created && timeMs(s.created) ? dateTimeUtc(timeMs(s.created) ?? 0) : "";
  const meta = `${search.done === "created" ? "Created just now." : created ? `Created ${created}.` : ""} ${s.mode === "PROVISIONED" ? `Provisioned capacity with ${count(s.shards, "open shard")}.` : "On-demand capacity."}`.trim();
  const sentence = streamSentence(s, consumers);
  const banner =
    search.done === "created" ? <ResultBanner title="Data stream created." returned={<>CreateStream returned 200. <b className="font-mono font-semibold text-bright">{d.name}</b> is {s.status === "ACTIVE" ? "active" : "being created"} with {count(s.shards, "open shard")}{s.mode === "ON_DEMAND" ? " (an on-demand stream starts with 4 here)" : ""}.</>} onDismiss={() => go({})} testId="created-banner" />
    : search.done === "tags" ? <ResultBanner title="Tags saved." returned="AddTagsToStream and RemoveTagsFromStream answered: the Tags section shows what the stream carries now." onDismiss={() => go({})} testId="tags-banner" />
    : search.done === "sharing" ? <ResultBanner title="Sharing policy saved." returned="PutResourcePolicy returned 200. GetResourcePolicy reads it back on this tab." onDismiss={() => go({ tab: "sharing" })} testId="sharing-banner" />
    : undefined;
  const tabs = [
    { id: "configuration", label: "Configuration", render: () => <><ConfigTab s={s} metrics={metrics} oldest={oldest.data ?? null} tags={tags.data} manage={() => go({ view: "tags" })} /><div className="mt-12"><RecentCalls calls={d.calls} noun="stream" title="Recent calls" scope={d.name} /></div></> },
    { id: "viewer", label: "Data Viewer", render: () => <ViewerTab s={s} oldest={oldest.data ?? null} /> },
    { id: "sharing", label: "Data stream sharing", render: () => <SharingTab s={s} create={() => go({ view: "sharing-create" })} /> },
  ];
  return (
    <>
      <ResourcePage
        detail={{ ...d, sentence: null, wired: [] }}
        meta={meta}
        banner={banner}
        overview={<>
          {tab === "configuration" && <p className="mb-6 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="detail-sentence"><Emphasis text={sentence} names={[d.name, ...consumers]} bold={[/\d[\d,]* (open shards?|registered consumers?)/]} /></p>}
          {tab === "configuration" && (feeders.length > 0 || consumers.length > 0) && <FlowStrip groups={[{ label: "Wired to", chips: feeders.map((label) => ({ label: `${label} (producer)` })) }, { chips: [{ label: d.name, self: true }] }, { chips: consumers.map((label) => ({ label })) }]} />}
          {tab !== "sharing" && <Summary s={s} />}
        </>}
        details={false}
        tabs={tabs}
        tab={tab}
        onTab={(id) => go(id === "configuration" ? {} : { tab: id })}
        actions={[{ label: "Delete data stream", onSelect: () => setDeleting(true) }]}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="stream-page"
      />
      <DeleteStream open={deleting} onOpenChange={setDeleting} name={d.name} onDone={() => { setDeleting(false); void navigate({ to: "/resources/$service/$second", params: { service: d.service, second: d.kind }, search: { gone: d.name } }); }} />
    </>
  );
}
