// SNS's subscriptions: the list across all topics (sns__subscriptions-list, subscriptions-none, subscription-delete). A subscription is chosen with its
// radio, and Edit and Delete act on it; one waiting for its endpoint cannot be deleted. `?create=yes` is Create subscription, as a page.
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { LinkIcon, PencilIcon, PlusIcon, TrashIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useDelete, useResourceList } from "../../../api/resources";
import { HINT } from "../index-page";
import { RadioRows, Rich, ServiceTabs } from "./parts";
import { subscriptionsSentence } from "./sentences";
import { DeleteSubscription } from "./sub-dialogs";
import { orderSubs, subRow } from "./sub-model";
import { subColumns } from "./subs-table";
import { SubCreate } from "./sub-create";
import { countWord } from "./protocols";

export function SubsList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string };
  if (search.create === "yes") return <SubCreate />;
  return <Subs service={service} kind={kind} gone={search.gone} />;
}

const COLUMNS = subColumns({ radio: true, topic: true });

function Subs({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const region = useAbout().data?.region ?? "this Region";
  const q = useResourceList(service, kind);
  const del = useDelete(service, kind);
  const [chosen, setChosen] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const { refetch } = q;
  const refresh = useCallback(() => void refetch(), [refetch]);
  const first = q.data?.pages[0];
  const rows = useMemo(() => orderSubs((q.data?.pages.flatMap((p) => p.rows) ?? []).map(subRow)), [q.data]);
  const pick = rows.find((r) => r.name === chosen) ?? null;
  const none = rows.length === 0;
  const waiting = [...new Set(rows.filter((r) => r.pending).map((r) => r.topic))].map((t) => [t, rows.filter((r) => r.pending && r.topic === t).length] as const);
  const go = (to: "create" | "edit") => {
    if (to === "create") void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: { create: "yes" } });
    else if (pick) void navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name: pick.name }, search: { view: "edit" } });
  };
  const sentence = subscriptionsSentence(rows);
  const why = pick?.pending ? "You can't delete a subscription that's pending confirmation. Select a subscription with a status of Confirmed." : undefined;
  return (
    <>
      <PageHeader
        title="Subscriptions"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub={`Every subscription in ${region}, across all topics.`}
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={() => go("create")} data-testid="create-open"><PlusIcon /> Create subscription</Button>}
      />
      <ServiceTabs active="subscriptions" />
      {gone && <ResultBanner className="mb-6" title={`Subscription ${gone} deleted.`} returned="Unsubscribe answered, and the subscription is gone from the list." left="The topic stops delivering to its endpoint; the endpoint itself is not touched." onDismiss={() => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: {} })} testId="deleted-banner" />}
      {q.isError && !first ? (
        <ErrorBlock what="The subscriptions could not be listed" error={q.error} hint={HINT} onRetry={refresh} retrying={q.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={LinkIcon} title="No subscriptions" action={<Button variant="outline" className="bg-card" onClick={() => go("create")}><PlusIcon /> Create subscription</Button>}>
            Nothing is subscribed to a topic in this region. Create a subscription to deliver a topic's messages to a queue, a function or an HTTP endpoint.
          </EmptyState>
        </div>
      ) : (
        <>
          <Rich parts={sentence} className="mb-6" testId="list-sentence" />
          <div className="mb-3 flex flex-wrap items-center justify-end gap-2" data-testid="sub-actions">
            <Button type="button" variant="outline" className="bg-card" disabled={!pick || pick.pending} title={pick?.pending ? "A subscription waiting for its endpoint has no ARN to change yet." : undefined} onClick={() => go("edit")} data-testid="action-edit"><PencilIcon /> Edit</Button>
            <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={!pick || pick.pending} title={why} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Delete</Button>
          </div>
          <RadioRows value={chosen} onChange={setChosen}>
            <DataTable aria-label="Subscriptions" data={rows} columns={COLUMNS} rowId={(r) => r.name} maxHeight={680} rowHeight={56} rowClassName={(r) => (r.name === chosen ? "[&>td]:bg-pick/50" : undefined)} onRowClick={(r) => setChosen(r.name)} />
          </RadioRows>
          <div className="mt-4 flex flex-wrap items-center justify-end gap-3 text-[13px] text-muted-foreground">
            <span data-testid="page-line">{q.hasNextPage ? `1 to ${rows.length} subscriptions, more to load` : `${rows.length} ${rows.length === 1 ? "subscription" : "subscriptions"}, one ListSubscriptions page (a page holds up to 100)`}</span>
            {q.hasNextPage && <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => void q.fetchNextPage()} disabled={q.isFetchingNextPage} data-testid="load-more">Load more</Button>}
          </div>
          <p className="mt-4 max-w-[48rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="delete-help">
            <b className="font-semibold text-foreground">You can't delete a subscription that's pending confirmation.</b> Select a subscription with a status of Confirmed.{waiting.length > 0 ? <> A pending one is confirmed from its topic's page: {waiting.map(([t, n], i) => <span key={t}>{i > 0 ? "; " : ""}<Link to="/resources/$service/$kind/$name" params={{ service: "sns", kind: "topics", name: t }} search={{ tab: "subscriptions" }} className="text-link hover:underline">{t}</Link> has {countWord(n).toLowerCase()}</span>)}.</> : null} On AWS, SNS deletes unconfirmed subscriptions after 48 hours.
          </p>
        </>
      )}
      {pick && !pick.pending && (
        <DeleteSubscription open={asking} onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }} protocol={pick.protocol} endpoint={pick.endpoint} topic={pick.topic} pending={del.isPending} error={del.error}
          onConfirm={() => del.mutate(pick.name, { onSuccess: () => { setAsking(false); setChosen(null); toast.success("Subscription deleted"); void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: { gone: pick.id?.slice(0, 8) ? `${pick.id.slice(0, 8)}…` : pick.name } }); } })} />
      )}
    </>
  );
}
