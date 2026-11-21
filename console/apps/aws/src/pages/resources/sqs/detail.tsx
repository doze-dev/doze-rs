// A queue's page (sqs__queue-detail, dlq-detail, queue-created, queue-purged and the tabs of the queue): the header with its verbs,
// the sentence, what it is wired to, the Details card and the tabs. Edit, Send and receive, Start DLQ redrive and the two
// forms that wire it to SNS and Lambda are pages of their own, chosen by `?view=`.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Callout } from "@doze/ui/components/app/callout";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useAbout } from "../../../api/queries";
import { useDelete, useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { Wired } from "../../../api/types/Wired";
import { ResourcePage, ResourcePageSkeleton, TagsTab, type Verb } from "../../../kit";
import { useOp } from "../deep/api";
import { HINT } from "../index-page";
import { timeMs } from "../model";
import { DeleteQueue, feeding, PurgeQueue } from "./dialogs";
import { ruleName, useFeeders } from "./feeders";
import { DetailsCard } from "./parts";
import { goneWords, queueOf, queueSentence } from "./queue-model";
import { latest, redriveBanner, taskSentence } from "./redrive-model";
import { useTasks } from "./redrive-status";
import { chipWords, madeSentence, purgedSentence, sourceSentence, UPSTREAM } from "./sentences";
import { LambdaTab, PolicyTab, SnsTab, type TabProps } from "./tabs";
import { DlqTab, EncryptionTab } from "./tabs-dlq";
import { SubPage } from "./sub-pages";

const SOURCES = "Queues that use it as their dead-letter queue";

type Notice = { kind: "purged"; count: number };

export function SqsDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => {
    void refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "deep", service, kind, name] });
  };
  if (detail.isError && !detail.data) return <ErrorBlock what="This queue could not be read" error={detail.error} hint={HINT} onRetry={refresh} retrying={detail.isFetching} testId="detail-error" />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <QueueView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

function QueueView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useSearch({ strict: false }) as { view?: string; done?: string };
  const navigate = useNavigate();
  const t = { service: d.service, kind: d.kind, name: d.name };
  const q = useMemo(() => queueOf(d), [d]);
  const about = useAbout().data;
  const [asked, setTab] = useUrlParam("tab");
  const [notice, setNotice] = useState<Notice | null>(null);
  const [dialog, setDialog] = useState<"delete" | "purge" | null>(null);
  const dead = d.subs.find((s) => s.title === SOURCES)?.rows.map((r) => r.name) ?? [];
  const feeders = useFeeders(t);
  const target = useResourceDetail("sqs", "queues", q.dlq ?? "", q.dlq !== null);
  const source = useResourceDetail("sqs", "queues", dead[0] ?? "", dead.length === 1);
  const tasks = useTasks(d, dead.length > 0);
  const del = useDelete("sqs", "queues");
  const purge = useOp(t, "PurgeQueue");
  const go = (view?: string, done?: string, tab?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: { ...(view ? { view } : {}), ...(done ? { done } : {}), ...(tab ? { tab } : {}) } });
  if (search.view) return <SubPage view={search.view} d={d} q={q} back={() => go()} done={(done, tab) => go(undefined, done, tab)} />;

  const fed = feeders.data ?? [];
  const holds = target.data ? (({ available, inFlight, delayed }) => available + inFlight + delayed)(queueOf(target.data)) : null;
  const total = q.available + q.inFlight + q.delayed;
  const task = latest(tasks.data?.Results);
  const fromSource = sourceSentence(d.sentence, dead, source.data ? queueOf(source.data).maxReceives : null);
  const sentence =
    notice?.kind === "purged" ? purgedSentence(q.name, q.available, q.inFlight)
    : search.done === "created" && total === 0 ? madeSentence(q.name, q.fifo)
    : dead.length > 0 ? (taskSentence(task, q.name, total) ?? fromSource)
    : queueSentence(d.sentence, q, holds);
  const wired: Wired[] = [
    ...fed.map((f) => ({ label: "Fed by", to: { service: "eventbridge", kind: "rules", name: ruleName(f) }, note: `The rule ${f.rule} on the ${f.bus} bus` })),
    ...(d.wired ?? []),
  ];
  const shown: ResourceDetail = { ...d, sentence, wired };
  const tabProps: TabProps = { d, q, go: (v) => go(v), account: about?.account ?? "", dead };
  const redrive = redriveBanner(search.done, tasks.isPending ? null : task, q.name, dead);
  const banner =
    search.done === "created" ? <ResultBanner title={`Queue ${q.name} created.`} returned="CreateQueue returned its URL, and the queue's Details page opens." onDismiss={() => go()} testId="created-banner" />
    : search.done === "updated" ? <ResultBanner title={`Queue ${q.name} updated.`} returned="SetQueueAttributes saved the settings that changed; tags went through TagQueue and UntagQueue." onDismiss={() => go()} testId="updated-banner" />
    : search.done === "trigger-created" ? <ResultBanner title="Lambda trigger configured." returned={`CreateEventSourceMapping made a mapping that polls ${q.name}.`} onDismiss={() => go(undefined, undefined, "lambda")} testId="trigger-banner" />
    : search.done === "subscribed" ? <ResultBanner title="Subscribed to the topic." returned={`Subscribe returned a subscription for ${q.name}, and the topic may send to it.`} onDismiss={() => go(undefined, undefined, "sns")} testId="subscribed-banner" />
    : redrive ? <ResultBanner title={redrive.title} returned={redrive.returned} onDismiss={() => go()} testId={redrive.title === "Redrive task started." ? "redrive-started-banner" : "redrive-completed-banner"} />
    : notice?.kind === "purged" ? <ResultBanner title={`${q.name} purged.`} returned={goneWords(notice.count)} left={<>AWS refuses a second purge inside 60 seconds (PurgeQueueInProgress); doze-aws does the same unless started with <span className="font-mono">--sqs-skip-60s-waits</span>.</>} tone="good" onDismiss={() => setNotice(null)} testId="purged-banner" />
    : undefined;
  const onVerb = (v: Verb) => {
    if (v.id === "edit") go("edit");
    else if (v.id === "messages") go("messages");
    else if (v.id === "redrive") go("redrive");
    else if (v.id === "delete") setDialog("delete");
    else if (v.id === "purge") setDialog("purge");
  };
  const tabs = [
    { id: "sns", label: "SNS subscriptions", render: () => <SnsTab {...tabProps} /> },
    { id: "lambda", label: "Lambda triggers", render: () => <LambdaTab {...tabProps} /> },
    { id: "dlq", label: "Dead-letter queue", render: () => <DlqTab {...tabProps} /> },
    { id: "tags", label: "Tagging", render: () => <TagsTab target={t} noun="tag" countNoun="cost allocation tag" note="Tags are case-sensitive and SQS reads them as plain strings. A new tag with the same key as an existing one overwrites it. Adding more than 50 tags to a queue isn't recommended." explain={`A tag is a key and a value you attach to a queue to organise it or track its cost.`} emptyAction={<Button type="button" variant="outline" className="bg-card" onClick={() => go("edit")} data-testid="tags-edit-queue">Edit queue</Button>} /> },
    { id: "policy", label: "Access policy", render: () => <PolicyTab {...tabProps} /> },
    { id: "encryption", label: "Encryption", render: () => <EncryptionTab {...tabProps} /> },
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={q.created ? <>Created {when(q.created)}. Last modified {when(q.modified ?? q.created)}.</> : undefined}
        banner={banner}
        overview={<><DetailsCard q={q} />{notice?.kind === "purged" && feeding(fed, "emptied", q.name) && <Callout tone="warn" className="mt-6" testId="purged-feeding"><Emphasis text={feeding(fed, "emptied", q.name) ?? ""} bold={fed.map((f) => f.rule)} /></Callout>}</>}
        tabs={tabs}
        tab={asked ?? "dlq"}
        onTab={(id) => setTab(id)}
        onVerb={onVerb}
        wiredSelf={wired.filter((w) => UPSTREAM.some((u) => w.label.startsWith(u))).length}
        wiredText={chipWords}
        verbStates={dead.length === 0 ? { redrive: { disabled: `${q.name} is not the dead-letter queue of any queue, so there is nothing to redrive.`, quiet: true } } : undefined}
        primary="messages"
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="queue-page"
      />
      <DeleteQueue open={dialog === "delete"} onOpenChange={(o) => { if (!o) { setDialog(null); del.reset(); } }} q={q} feeders={fed} dlqHolds={holds} pending={del.isPending} error={del.error}
        onConfirm={() => del.mutate(d.name, { onSuccess: () => { setDialog(null); void navigate({ to: "/resources/$service/$second", params: { service: d.service, second: d.kind }, search: { gone: d.name, ...(q.dlq ? { from: q.dlq } : {}) } }); } })} />
      <PurgeQueue open={dialog === "purge"} onOpenChange={(o) => { if (!o) { setDialog(null); purge.reset(); } }} q={q} pending={purge.isPending} error={purge.error}
        onConfirm={() => purge.mutate({}, { onSuccess: () => { setDialog(null); setNotice({ kind: "purged", count: total }); } })} />
    </>
  );
}

const when = (v: string): string => {
  const ms = timeMs(v);
  return ms === null ? v : dateTimeUtc(ms);
};
