// An alarm's page (cloudwatch__alarm-detail, alarm-history, alarm-tags, alarm-tags-none): the header with its state and type, the
// description as its lead, and the tabs Details (the graph, the details, the actions), History and Tags.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useState } from "react";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { ApiFailure } from "@doze/ui/lib/api";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, PlainConfirm, RecentCalls, ResourcePage, ResourcePageSkeleton, TagsTab } from "../../../kit";
import { ActionsCard, DetailsCard } from "./alarm-cards";
import { AlarmChart } from "./alarm-chart";
import { AlarmHistory } from "./alarm-history";
import { useAlarm, useDeleteAlarms, useHistory } from "./api";
import { type Alarm, toMs } from "./model";
import { deleteWords, watching } from "./words";

const day = (ms: number) => new Date(ms).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });
const WORD: Record<string, string> = { ALARM: "ALARM", OK: "OK", INSUFFICIENT_DATA: "Insufficient data" };

export function AlarmDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const read = useAlarm(name, detail.isSuccess);
  const refresh = () => { void detail.refetch(); void read.refetch(); };
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={refresh} retrying={detail.isFetching} />;
  if (!detail.data || read.alarm === undefined) return <ResourcePageSkeleton />;
  if (read.alarm === null) return <DetailError service={service} kind={kind} name={name} error={new ApiFailure(404, { code: "NOT_FOUND", message: `There is no alarm named ${name}.` })} />;
  return <AlarmView d={detail.data} a={read.alarm} updatedAt={Math.max(detail.dataUpdatedAt, read.dataUpdatedAt)} fetching={detail.isFetching || read.isFetching} refresh={refresh} />;
}

function AlarmView({ d, a, updatedAt, fetching, refresh }: { d: ResourceDetail; a: Alarm; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { done?: string };
  const history = useHistory(a.AlarmName);
  const del = useDeleteAlarms();
  const [asking, setAsking] = useState(false);
  const since = toMs(a.StateUpdatedTimestamp);
  const created = toMs(a.AlarmConfigurationUpdatedTimestamp);
  const sentence = a.AlarmDescription ? `${a.AlarmDescription} ${a.StateValue} since ${since ? dateTimeUtc(since) : "creation"}.` : created ? `Created ${day(created)}.` : null;
  const shown: ResourceDetail = {
    ...d,
    sentence,
    wired: [],
    verbs: [],
    badges: [{ text: WORD[a.StateValue] ?? a.StateValue, tone: a.StateValue === "ALARM" ? "bad" : a.StateValue === "OK" ? "good" : "warn" }, { text: a.AlarmRule !== undefined ? "Composite alarm" : "Metric alarm", tone: "neutral" }],
  };
  const edit = () => void navigate({ to: "/resources/$service/$second", params: { service: "cloudwatch", second: "alarms" }, search: { do: "create", name: a.AlarmName } });
  const calls = <RecentCalls calls={d.calls} noun="alarm" className="mt-10" />;
  const tabs = [
    { id: "details", label: "Details", render: () => (
      <div className="grid gap-6">
        {(a.MetricName || a.Metrics) && <AlarmChart alarm={a} />}
        <div className="grid items-start gap-6 xl:grid-cols-2"><DetailsCard a={a} /><ActionsCard a={a} history={history.items} /></div>
        {calls}
      </div>
    ) },
    { id: "history", label: "History", render: () => <><AlarmHistory name={a.AlarmName} />{calls}</> },
    { id: "tags", label: "Tags", render: () => (
      // The board's Manage tags is a card of its own, with no Tags heading over it (the heading goes while the manager is open).
      <div className="grid gap-4 [&:has([data-testid=tags-lead])>h2:first-child]:hidden">
        <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Tags</h2>
        <TagsTab target={{ service: d.service, kind: d.kind, name: d.name }} noun="tag" explain="Tags are key-value labels for organising alarms." manage={{ open: "Manage tags", title: "Manage tags", lead: "Tags are key-value pairs that help you organize and categorize your alarms. You can also use them to scope user permissions.", rules: <><b className="font-semibold text-foreground">Up to 50 tags per alarm; keys are 1 to 128 characters and values 0 to 256.</b> Saving new keys calls TagResource; removed rows call UntagResource. A tag with an existing key replaces its value.</> }} />
        {calls}
      </div>
    ) },
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        banner={search.done === "updated" ? <ResultBanner title={`Alarm ${a.AlarmName} updated.`} returned="PutMetricAlarm replaced the alarm's settings; its state and history are kept." onDismiss={() => void navigate({ to: ".", search: {}, replace: true })} testId="updated-banner" /> : search.done === "created" ? <ResultBanner title={`Alarm ${a.AlarmName} created.`} returned="PutMetricAlarm made the alarm. Its state is INSUFFICIENT_DATA until the first evaluation." onDismiss={() => void navigate({ to: ".", search: {}, replace: true })} testId="created-banner" /> : undefined}
        bannerAbove
        tabs={tabs}
        details={false}
        actions={[{ label: "Edit", onSelect: edit }, { label: "Delete", onSelect: () => setAsking(true) }]}
        sentenceBold={[/\b\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\b/]}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="alarm-page"
      />
      <PlainConfirm
        open={asking}
        onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }}
        title={`Delete ${a.AlarmName}?`}
        description={<Emphasis text={deleteWords([a])} names={[a.AlarmName, watching(a)]} />}
        call="DeleteAlarms"
        pending={del.isPending}
        error={del.error}
        testId="delete-dialog"
        onConfirm={() => del.mutate([a.AlarmName], { onSuccess: () => { setAsking(false); void navigate({ to: "/resources/$service/$second", params: { service: "cloudwatch", second: "alarms" }, search: { gone: a.AlarmName } }); } })}
      />
    </>
  );
}
