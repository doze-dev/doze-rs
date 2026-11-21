// SNS's topics: the list (sns__topics-list, topics-none, and the list a deletion lands on, topic-deleted). A topic is chosen with its
// radio, and Edit, Delete and Publish message act on it. `?create=yes` is Create topic, as a page.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronLeftIcon, ChevronRightIcon, PencilIcon, PlusIcon, SearchIcon, SendIcon, TrashIcon, WaypointsIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useDelete, useResourceList } from "../../../api/resources";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { LastCalls, RadioRows, radioColumn, Rich, ServiceTabs, snsCalls, useSnsCalls } from "./parts";
import { topicsLeftSentence, topicsSentence } from "./sentences";
import { subRow } from "./sub-model";
import { TopicCreate } from "./topic-create";
import { DeleteTopic } from "./topic-dialogs";
import { clock } from "./words";

const RADIO = radioColumn<ResourceRow>((r) => r.name, "topic");

const COLUMNS: DataColumn<ResourceRow>[] = [
  RADIO,
  { id: "name", header: "Name", accessorFn: (r) => r.name.toLowerCase(), size: 280, meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "sns", kind: "topics", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
  { id: "type", header: "Type", size: 120, accessorFn: (r) => r.cells[0] ?? "", cell: (c) => <Badge variant="outline" className="font-normal text-muted-foreground">{c.row.original.cells[0] === "FIFO" ? "FIFO" : "Standard"}</Badge> },
  { id: "arn", header: "ARN", accessorFn: (r) => r.id, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.id}</span> },
];

export function TopicsList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string };
  if (search.create === "yes") return <TopicCreate />;
  return <Topics service={service} kind={kind} gone={search.gone} />;
}

function Topics({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const region = useAbout().data?.region ?? "this Region";
  const list = useResourceList(service, kind);
  const subs = useResourceList("sns", "subscriptions");
  const calls = useSnsCalls();
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const del = useDelete(service, kind);
  const { refetch } = list;
  const refresh = useCallback(() => {
    void refetch();
    void subs.refetch();
    void calls.refetch();
  }, [refetch, subs, calls]);
  const first = list.data?.pages[0];
  const all = useMemo(() => list.data?.pages.flatMap((p) => p.rows) ?? [], [list.data]);
  const rows = useMemo(() => all.filter((r) => r.name.toLowerCase().includes(typed.trim().toLowerCase())), [all, typed]);
  const pick = all.find((r) => r.name === chosen) ?? null;
  const subRows = useMemo(() => (subs.data?.pages.flatMap((p) => p.rows) ?? []).map(subRow), [subs.data]);
  const recent = snsCalls(calls.data);
  const publish = recent.find((c) => c.action === "Publish" && !c.error_code && c.status < 400);
  const last = publish && publish.resource ? { topic: publish.resource.split(":").pop() ?? publish.resource, at: clock(publish.at_ms) } : null;
  const none = all.length === 0;
  const fifo = all.find((r) => r.cells[0] === "FIFO");
  const go = (to: "create" | "edit" | "publish") => {
    if (to === "create") void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: { create: "yes" } });
    else if (pick) void navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name: pick.name }, search: { view: to } });
  };
  const sentence = gone ? topicsLeftSentence(all.length, region, gone) : topicsSentence(all.length, subRows, region, last);
  return (
    <>
      <PageHeader
        title="Topics"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub="An Amazon SNS topic is a logical access point that acts as a communication channel."
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={() => go("create")} data-testid="create-open"><PlusIcon /> Create topic</Button>}
      />
      <ServiceTabs active="topics" />
      {gone && <ResultBanner className="mb-6" title={`Topic ${gone} deleted.`} returned="DeleteTopic answered, and the topic is gone from the list." left="Its subscriptions went with it; the queues, functions and endpoints they delivered to are not touched." onDismiss={() => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: {} })} testId="deleted-banner" />}
      {list.isError && !first ? (
        <ErrorBlock what="The topics could not be listed" error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <Rich parts={sentence} className="mb-6" testId="list-sentence" />
          <EmptyState icon={WaypointsIcon} title="No topics" action={<Button variant="outline" className="bg-card" onClick={() => go("create")}><PlusIcon /> Create topic</Button>}>
            A topic lets you group multiple endpoints, such as AWS Lambda, Amazon SQS or HTTP/S endpoints, so one publish reaches all of them.
          </EmptyState>
          <p className="mt-6 max-w-[40rem] border-l-2 border-input pl-3.5 text-[13.5px] leading-[21px] text-muted-foreground"><b className="font-semibold text-bright">An account can create at most 100,000 standard topics and 1,000 FIFO topics.</b> CreateTopic is idempotent: creating a name you already own returns that topic's ARN.</p>
        </div>
      ) : (
        <>
          <Rich parts={sentence} className="mb-6" testId="list-sentence" />
          <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
            <div className="relative w-full max-w-[22rem]">
              <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search topics" aria-label="Search topics" className="bg-card pl-9" data-testid="list-search" />
            </div>
            <div className="flex flex-wrap items-center gap-2" data-testid="topic-actions">
              <Button type="button" variant="outline" className="bg-card" disabled={!pick} onClick={() => go("edit")} data-testid="action-edit"><PencilIcon /> Edit</Button>
              <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={!pick} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Delete</Button>
              <Button type="button" variant="outline" className="bg-card" disabled={!pick} onClick={() => go("publish")} data-testid="action-publish"><SendIcon /> Publish message</Button>
            </div>
          </div>
          <RadioRows value={chosen} onChange={setChosen}>
            <DataTable aria-label="Topics" data={rows} columns={COLUMNS} rowId={(r) => r.name} maxHeight={680} rowClassName={(r) => (r.name === chosen ? "[&>td]:bg-accent" : undefined)} onRowClick={(r) => setChosen(r.name)} empty={`No topic's name contains “${typed}”.`} />
          </RadioRows>
          <p className="mt-3 max-w-[40rem] text-[13px] leading-5 text-muted-foreground" data-testid="publish-help">
            Publish message acts on the selected topic.{fifo ? <> With <ResourceTo to={{ service, kind, name: fifo.name }}>{fifo.name}</ResourceTo> selected instead, it opens the Publish message page of the FIFO topic.</> : null}
          </p>
          <div className="mt-4 flex flex-wrap items-center justify-end gap-3 text-[13px] text-muted-foreground"><span data-testid="page-line">{list.hasNextPage ? `1 to ${rows.length} topics, more to load` : `${rows.length} ${rows.length === 1 ? "topic" : "topics"}, one ListTopics page (a page holds up to 100)`}</span><Button type="button" variant="outline" size="sm" className="bg-card" disabled aria-label="Previous page" data-testid="page-previous"><ChevronLeftIcon /> Previous</Button><Button type="button" variant="outline" size="sm" className="bg-card" disabled={!list.hasNextPage || list.isFetchingNextPage} onClick={() => void list.fetchNextPage()} data-testid="load-more">Next <ChevronRightIcon /></Button></div>
          <div className="mt-12"><LastCalls calls={recent} /></div>
        </>
      )}
      {pick && (
        <DeleteTopic open={asking} onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }} name={pick.name} arn={pick.id} pending={del.isPending} error={del.error}
          onConfirm={() => del.mutate(pick.name, { onSuccess: () => { setAsking(false); setChosen(null); void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: { gone: pick.name } }); } })} />
      )}
    </>
  );
}
