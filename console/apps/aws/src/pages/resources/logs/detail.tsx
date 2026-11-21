// A log group's page (logs__log-group-detail, log-group-created, log-streams-none, metric-filters, metric-filters-none): the header with Search
// log group and the Actions, the sentence, the strip from what writes to what it feeds, and the tabs. Search, the metric filter and the
// subscription filter are pages of their own, chosen by `?view=`.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { lazy, Suspense, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { clockUtc, dateTimeUtc } from "@doze/ui/lib/format";
import { useDelete, useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { FlowStrip, type FlowChip, PlainConfirm, RecentCalls, ResourcePage, ResourcePageSkeleton, type Verb, useFeeders } from "../../../kit";
import { callOp, on } from "../deep/api";
import { hintFor } from "../index-page";
import { timeMs } from "../model";
import { FiltersTab, StreamsTab, SubscriptionsTab } from "./group-tabs";
import { activityWords, filtersOf, groupOf, streamsOf, subscriptionsOf } from "./model";
import { RetentionDialog } from "./retention-dialog";
import { retentionWords } from "./model";

const Search = lazy(() => import("./search-page").then((m) => ({ default: ({ d }: { d: ResourceDetail }) => <m.GroupSearch group={d.name} /> })));
const Tail = lazy(() => import("./tail-page").then((m) => ({ default: ({ d }: { d: ResourceDetail }) => <m.TailPage group={d.name} /> })));
const Filter = lazy(() => import("./metric-filter").then((m) => ({ default: ({ d }: { d: ResourceDetail }) => <m.MetricFilterCreate group={d.name} /> })));
const Subscribe = lazy(() => import("./subscription").then((m) => ({ default: ({ d }: { d: ResourceDetail }) => <m.SubscriptionCreate group={d.name} /> })));

export function GroupDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => { void refetch(); void qc.invalidateQueries({ queryKey: ["resources", "deep", "logs", "groups", name] }); };
  if (detail.isError && !detail.data) return <ErrorBlock what="This log group could not be read" error={detail.error} hint={hintFor(detail.error, "log group")} onRetry={refresh} retrying={detail.isFetching} testId="detail-error" />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <GroupView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

/** `lambda:function:send-email` → a link to the function; a Kinesis stream the same; the rest by what follows the last colon. */
function destinationChip(arn: string): FlowChip {
  const [, , service, , , type, name] = arn.split(":");
  if (service === "lambda" && type === "function" && name) return { label: `Lambda ${name}`, to: { service: "lambda", kind: "functions", name } };
  if (service === "kinesis" && arn.includes(":stream/")) return { label: `Kinesis ${arn.split("/").pop() ?? arn}`, to: { service: "kinesis", kind: "streams", name: arn.split("/").pop() ?? arn } };
  return { label: arn.split(":").pop() ?? arn, title: arn };
}

function GroupView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useSearch({ strict: false }) as { view?: string; done?: string; tab?: string; name?: string };
  const navigate = useNavigate();
  const g = useMemo(() => groupOf(d.name, d.fields), [d]);
  const streams = useMemo(() => streamsOf(d.subs.find((s) => s.title === "Log streams")), [d]);
  const streamsMore = d.subs.find((s) => s.title === "Log streams")?.more ?? false;
  const filters = useMemo(() => filtersOf(d.subs.find((s) => s.title === "Metric filters")), [d]);
  const subs = useMemo(() => subscriptionsOf(d.subs.find((s) => s.title === "Subscription filters")), [d]);
  const [retention, setRetention] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const del = useDelete("logs", "groups");
  const feeders = useFeeders(d.calls, ["PutLogEvents"]);
  const hour = useQuery({
    queryKey: ["resources", "deep", "logs", "groups", d.name, "last-hour", updatedAt],
    queryFn: ({ signal }) => callOp<{ events?: { logStreamName?: string }[]; nextToken?: string }>(on.group(d.name), "FilterLogEvents", { logGroupName: d.name, startTime: Date.now() - 3_600_000, limit: 10000 }, signal),
    enabled: streams.length > 0 && !search.view,
    retry: false,
  });
  const go = (patch: { view?: string; tab?: string; done?: string; name?: string }) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: patch });
  const view = search.view;
  if (view === "search" || view === "tail" || view === "metric-filter" || view === "subscription") {
    const Page = view === "search" ? Search : view === "tail" ? Tail : view === "metric-filter" ? Filter : Subscribe;
    return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}><Page d={d} /></Suspense>;
  }
  const tab = search.tab === "metric-filters" || search.tab === "subscription-filters" ? search.tab : "streams";
  const created = search.done === "created";
  const newest = streams[0];
  const activity = hour.data ? { events: hour.data.events?.length ?? 0, streams: new Set((hour.data.events ?? []).map((e) => e.logStreamName)).size, more: Boolean(hour.data.nextToken) } : null;
  const sentence = activityWords(d.name, activity, newest?.last ? { at: clockUtc(newest.last), stream: newest.name } : null, streams.length);
  const make = filters.find((f) => f.name === search.name);
  const banner =
    created ? <ResultBanner title="Log group created." returned={<>CreateLogGroup returned 200. <b className="font-mono font-semibold text-bright">{d.name}</b> is empty and has no retention policy yet.</>} onDismiss={() => go({})} testId="created-banner" />
    : search.done === "filter-created" ? <ResultBanner title="Metric filter created." returned={<>PutMetricFilter returned 200. <b className="font-mono font-semibold text-bright">{search.name}</b> now turns matching events into {make ? <><b className="font-mono font-semibold text-bright">{make.metric}</b> in <b className="font-mono font-semibold text-bright">{make.namespace}</b></> : "a metric"}.</>} onDismiss={() => go({ tab: "metric-filters" })} testId="filter-created-banner" />
    : search.done === "subscription-created" ? <ResultBanner title="Subscription filter created." returned={<>PutSubscriptionFilter returned 200. <b className="font-mono font-semibold text-bright">{search.name}</b> sends matching events to its function as they arrive.</>} onDismiss={() => go({ tab: "subscription-filters" })} testId="subscription-created-banner" />
    : undefined;
  const meta = <>{created ? "Created just now." : g.created ? `Created ${timeMs(g.created) ? dateTimeUtc(timeMs(g.created) ?? 0) : g.created}.` : ""} {g.retention === null ? "No retention policy: events are kept until doze-aws's --logs-retention-days (7 by default)." : `Events are kept ${retentionWords(g.retention)}.`}</>;
  const strip: { label?: string; chips: FlowChip[] }[] = [
    { label: "Wired to", chips: feeders.map((label) => ({ label })) },
    { chips: [{ label: d.name, self: true }] },
    { chips: [...filters.map((f) => ({ label: `metric ${f.namespace} ${f.metric}` })), ...subs.map((s) => destinationChip(s.destination))] },
  ];
  const tabs = [
    { id: "streams", label: "Log streams", count: streams.length, render: () => <><StreamsTab g={g} streams={streams} more={streamsMore} justMade={created} retention={() => setRetention(true)} group={d.name} back={() => void navigate({ to: "/resources/$service/$second", params: { service: d.service, second: d.kind }, search: {} })} />{streams.length > 0 && <div className="mt-12"><RecentCalls calls={d.calls} noun="log group" title="Recent calls" scope={d.name} /></div>}</> },
    { id: "metric-filters", label: "Metric filters", count: filters.length, render: () => <><FiltersTab group={d.name} filters={filters} create={() => go({ view: "metric-filter" })} />{filters.length > 0 && <div className="mt-12"><RecentCalls calls={d.calls} noun="log group" title="Recent calls" scope={d.name} /></div>}</> },
    ...(subs.length > 0 ? [{ id: "subscription-filters", label: "Subscription filters", count: subs.length, render: () => <SubscriptionsTab group={d.name} subs={subs} create={() => go({ view: "subscription" })} /> }] : []),
  ];
  const onVerb = (v: Verb) => { if (v.id === "search") go({ view: "search" }); };
  return (
    <>
      <ResourcePage
        detail={{ ...d, sentence: null, wired: [] }}
        meta={meta}
        banner={banner}
        overview={<>
          <p className="mb-6 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="detail-sentence"><Emphasis text={sentence} names={[d.name, ...streams.map((s) => s.name)]} bold={[/\d[\d,]*\+? events?(?= in the last hour)/, /\d+ streams?/]} /></p>
          {(feeders.length > 0 || filters.length > 0 || subs.length > 0) && <FlowStrip groups={strip} />}
        </>}
        details={false}
        tabs={tabs}
        tab={tab}
        onTab={(id) => go(id === "streams" ? {} : { tab: id })}
        onVerb={onVerb}
        actions={[
          { label: "Create metric filter", onSelect: () => go({ view: "metric-filter" }) },
          { label: "Subscription filters (Lambda)", onSelect: () => go({ view: "subscription" }) },
          { label: "Retention setting", onSelect: () => setRetention(true) },
          { label: "Live Tail", onSelect: () => go({ view: "tail" }) },
          { label: "Delete log group", onSelect: () => { del.reset(); setDeleting(true); } },
        ]}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="group-page"
      />
      <RetentionDialog group={retention ? { name: d.name, retention: g.retention } : null} onClose={() => setRetention(false)} />
      <PlainConfirm open={deleting} onOpenChange={setDeleting} title="Delete log group" call="DeleteLogGroup" pending={del.isPending} error={del.error} testId="delete-dialog"
        description={<>DeleteLogGroup deletes <b className="font-mono font-semibold text-bright">{d.name}</b> and permanently deletes all the archived log events associated with it.</>}
        onConfirm={() => del.mutate(d.name, { onSuccess: () => { setDeleting(false); void navigate({ to: "/resources/$service/$second", params: { service: d.service, second: d.kind }, search: { gone: d.name } }); } })} />
    </>
  );
}
