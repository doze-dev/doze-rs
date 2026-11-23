// A topic's page (sns__topic-detail, topic-alerts, topic-created, topic-published, topic-subscriptions-none and the tabs of the topic): the header with
// its verbs, the sentence of the tab, the strip from publisher to endpoints, the Details card and the tabs. Edit and Publish message are pages of
// their own, chosen by `?view=`.
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { lazy, Suspense, useMemo, useState } from "react";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { useAbout } from "../../../api/queries";
import { useDelete, useResourceDetail, useResourceList } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, RecentCalls, ResourcePage, ResourcePageSkeleton, TagsTab, type Verb, useTags } from "../../../kit";
import { ConfirmSubscription } from "./confirm-dialog";
import { Block, Rich } from "./parts";
import { lastPublished, publishedSentence } from "./publish-result";
import { rawWords } from "./sentences";
import { useSubscriptions } from "./sub-reads";
import { DeleteTopic } from "./topic-dialogs";
import { DetailsCard, Pipeline } from "./topic-parts";
import { accessSentence, archiveSentence, encryptionSentence, loggingSentence, retrySentence, tagsSentence, topicSentence } from "./topic-sentences";
import { SubscriptionsTab, topicSubs } from "./topic-subs-tab";
import { AccessTab, ArchiveTab, EncryptionTab, LoggingTab, RetryTab } from "./topic-tabs";
import { PublishedCall } from "./published-panel";
import { topicOf } from "./topic-model";

const Edit = lazy(() => import("./topic-edit").then((m) => ({ default: m.TopicEdit })));
const Publish = lazy(() => import("./publish").then((m) => ({ default: m.Publish })));

export function TopicDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => {
    void refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "tags", service, kind, name] });
    void qc.invalidateQueries({ queryKey: ["resources", "one", "sns", "subscriptions"] });
  };
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={refresh} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <TopicView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

type Dialog = "delete" | null;

function TopicView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useSearch({ strict: false }) as { view?: string; done?: string };
  const navigate = useNavigate();
  const about = useAbout().data;
  const account = about?.account ?? "";
  const t = useMemo(() => topicOf(d), [d]);
  const target = { service: d.service, kind: d.kind, name: d.name };
  const subs = useMemo(() => topicSubs(d, t.arn), [d, t.arn]);
  const raw = useSubscriptions(subs);
  const queues = useResourceList("sqs", "queues");
  const tags = useTags(target);
  const [asked, setTab] = useUrlParam("tab");
  const [dialog, setDialog] = useState<Dialog>(null);
  const [confirming, setConfirming] = useState<ReturnType<typeof topicSubs>[number] | null>(null);
  const del = useDelete("sns", "topics");
  const go = (view?: string, done?: string, tab?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: { ...(view ? { view } : {}), ...(done ? { done } : {}), ...(tab ? { tab } : {}) } });
  if (search.view === "edit" || search.view === "publish") {
    const props = { d, t, subs, back: () => go(), done: (done: string, tab?: string) => go(undefined, done, tab) };
    return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}>{search.view === "edit" ? <Edit {...props} /> : <Publish {...props} />}</Suspense>;
  }
  const rawText = raw.ready ? rawWords(raw.subs.map((s) => s.raw), raw.subs.map((s) => s.protocol)) : null;
  const fifoQueue = t.fifo ? ((queues.data?.pages.flatMap((p) => p.rows) ?? []).find((r) => r.name === t.name)?.name ?? null) : null;
  const tab = asked ?? "subscriptions";
  const published = search.done === "published" ? lastPublished(d.name) : null;
  const sentence =
    published ? publishedSentence(published, subs, raw.subs)
    : tab === "access" ? accessSentence(t.name, t.policy, account)
    : tab === "retry" ? retrySentence(t.name, t.own, subs.filter((s) => s.protocol === "http" || s.protocol === "https").length)
    : tab === "logging" ? loggingSentence(t)
    : tab === "encryption" ? encryptionSentence(t.name, t.kms)
    : tab === "tags" ? tagsSentence(t.name, (tags.data?.tags ?? []).map((x) => x.key))
    : tab === "archive" ? archiveSentence(t.name, t.archive)
    : topicSentence(t, subs, rawText, search.done === "created" ? { queue: fifoQueue } : null);
  const banner =
    search.done === "created" ? <ResultBanner title={`Topic ${t.name} created.`} returned="CreateTopic returned its ARN, and the topic's page opens. The Name, ARN, Display name and Topic owner are in the Details section." onDismiss={() => go()} testId="created-banner" />
    : search.done === "updated" ? <ResultBanner title={`Topic ${t.name} updated.`} returned="SetTopicAttributes saved each setting that changed, one call each; tags went through TagResource and UntagResource." onDismiss={() => go()} testId="updated-banner" />
    : search.done === "published" ? <ResultBanner title="Message published." returned={published ? <>Publish returned a MessageId{published.sequenceNumber ? " and a SequenceNumber" : ""}, and the topic's Details page opens.</> : "Publish returned a MessageId, and the topic's Details page opens."} onDismiss={() => go()} testId="published-banner" />
    : search.done === "subscribed" ? <ResultBanner title="Subscription created." returned={`Subscribe returned the subscription of ${t.name}.`} onDismiss={() => go(undefined, undefined, "subscriptions")} testId="subscribed-banner" />
    : search.done === "confirmed" ? <ResultBanner title="Subscription confirmed." returned="ConfirmSubscription returned its ARN: the endpoint now receives what is published." onDismiss={() => go(undefined, undefined, "subscriptions")} testId="confirmed-banner" />
    : undefined;
  const onVerb = (v: Verb) => {
    if (v.id === "edit") go("edit");
    else if (v.id === "publish") go("publish");
    else if (v.id === "delete") setDialog("delete");
  };
  const edit = () => go("edit");
  const http = subs.filter((s) => s.protocol === "http" || s.protocol === "https").length;
  const tabs = [
    { id: "subscriptions", label: "Subscriptions", render: () => <><SubscriptionsTab d={d} arn={t.arn} fifo={t.fifo} create={() => void navigate({ to: "/resources/$service/$second", params: { service: "sns", second: "subscriptions" }, search: { create: "yes", from: t.arn } })} confirm={setConfirming} /><div className="mt-12"><RecentCalls calls={d.calls} noun="topic" title={`Last calls that touched ${d.name}`} scope={d.name} pin={search.done === "published" ? ["Publish"] : search.done === "subscribed" ? ["Subscribe"] : undefined} /></div></> },
    { id: "access", label: "Access policy", render: () => <AccessTab t={t} edit={edit} /> },
    ...(t.fifo ? [] : [{ id: "retry", label: "Delivery retry policy (HTTP/S)", render: () => <RetryTab t={t} edit={edit} http={http} /> }]),
    { id: "logging", label: "Delivery status logging", render: () => <LoggingTab t={t} edit={edit} /> },
    { id: "encryption", label: "Encryption", render: () => <EncryptionTab t={t} edit={edit} calls={d.calls} /> },
    { id: "tags", label: "Tags", render: () => <Block title="Tags" lede="Metadata attached to the topic. Read with ListTagsForResource." testId="tags-tab"><TagsTab target={target} noun="tag" note="Key 1 to 128 characters, value 0 to 256. A topic cannot take more than 50 tags (TagLimitExceeded). Tags are accessible to other AWS services, billing included, so keep sensitive data out of them." explain="Tags are key-value labels for organising topics and tracking their cost." /></Block> },
    ...(t.fifo ? [{ id: "archive", label: "Archive policy", render: () => <ArchiveTab t={t} edit={edit} /> }] : []),
  ];
  const shown: ResourceDetail = { ...d, sentence: null, wired: [] };
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={t.fifo ? "A FIFO topic. Messages in a message group are delivered in the order they are published." : undefined}
        banner={banner}
        overview={<>
          {sentence && <Rich parts={sentence} className="mb-6" testId="detail-sentence" />}
          <Pipeline name={t.name} calls={d.calls} subs={subs} />
          <div className={published ? "mb-10 grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_22rem]" : "mb-10"}>
            <DetailsCard t={t} />
            {published && <PublishedCall published={published} />}
          </div>
        </>}
        tabs={tabs}
        tab={tab}
        onTab={(id) => setTab(id)}
        onVerb={onVerb}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="topic-page"
      />
      <DeleteTopic open={dialog === "delete"} onOpenChange={(o) => { if (!o) { setDialog(null); del.reset(); } }} name={t.name} arn={t.arn} pending={del.isPending} error={del.error}
        onConfirm={() => del.mutate(d.name, { onSuccess: () => { setDialog(null); void navigate({ to: "/resources/$service/$second", params: { service: d.service, second: d.kind }, search: { gone: d.name } }); } })} />
      <ConfirmSubscription sub={confirming} topic={t.name} onClose={() => setConfirming(null)} onDone={() => { setConfirming(null); refresh(); go(undefined, "confirmed", "subscriptions"); }} />
    </>
  );
}
