// The page of one archive, one replay or one API destination (eventbridge__archive-create, replay-start, api-destination-create, and what they
// open): the generic page's fields and recent calls under the kit's header, with the verbs the resource has. An archive is replayed from its
// page (Replay opens Start replay with the archive chosen); a replay that is Starting or Running is cancelled from its page; an archive and an
// API destination are deleted from theirs.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useState } from "react";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, PlainConfirm, RecentCalls, ResourcePage, ResourcePageSkeleton, type Verb, callKindOp, useDeleteClearing } from "../../../kit";
import { DetailFields } from "../../../kit/resource-parts";
import { REPLAYS } from "./api";
import { ownBanner } from "./own-words";

export function EbOneDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <OneView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refetch={() => void detail.refetch()} />;
}

function OneView({ d, updatedAt, fetching, refetch }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refetch: () => void }) {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { done?: string };
  const [deleting, setDeleting] = useState(false);
  const [cancelled, setCancelled] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const del = useDeleteClearing({ service: d.service, kind: d.kind, name: d.name });
  const banner = ownBanner(d.kind, d.name, search.done, cancelled !== null);
  const go = (to: string, s: Record<string, string>, params?: Record<string, string>) => void navigate({ to: to as never, params: params as never, search: s as never });
  const list = (s: Record<string, string> = {}) => go("/resources/$service/$second", s, { service: d.service, second: d.kind });
  const cancel = async () => {
    setRefused(null);
    try {
      await callKindOp(REPLAYS, "CancelReplay", { ReplayName: d.name });
      setCancelled(d.name);
      refetch();
    } catch (e) {
      setRefused((e as Error).message);
    }
  };
  const onVerb = (v: Verb) => {
    if (v.id === "replay") go("/resources/$service/$second", { create: "yes", name: d.name }, { service: "eventbridge", second: "replays" });
    else if (v.id === "cancel") void cancel();
    else if (v.id === "delete") setDeleting(true);
  };
  const one = d.one;
  return (
    <>
      <ResourcePage
        detail={{ ...d, wired: [] }}
        banner={refused ? <ResultBanner tone="warn" title={`CancelReplay was refused.`} returned={refused} onDismiss={() => setRefused(null)} testId="cancel-refused" /> : banner ? <ResultBanner title={banner.title} returned={banner.returned} onDismiss={() => go("/resources/$service/$kind/$name", {}, { service: d.service, kind: d.kind, name: d.name })} testId="done-banner" /> : undefined}
        onVerb={onVerb}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refetch} interval={false} />}
        overview={<><DetailFields detail={d} /><RecentCalls className="mt-12" calls={d.calls} noun={one} title="Recent calls" scope={d.name} /></>}
        testId={`${d.kind}-page`}
      />
      {d.delete && (
        <PlainConfirm
          open={deleting}
          onOpenChange={(o) => { setDeleting(o); if (!o) del.reset(); }}
          title={`Delete ${one}?`}
          description={<>Delete <b className="font-mono font-semibold text-bright">{d.name}</b>? {d.delete.words}</>}
          call={d.delete.operation}
          pending={del.isPending}
          error={del.error}
          onConfirm={() => del.mutate({ clear: false }, { onSuccess: () => list({ gone: d.name }) })}
          testId={`${d.kind}-delete`}
        />
      )}
    </>
  );
}
