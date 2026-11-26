// EventBridge's replays (eventbridge__replays-list, replays-none): the window, the last event replayed and when it started; a replay that is
// Starting or Running can be cancelled. `?create=yes` is Start new replay (`?name=<archive>` the one to replay).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, RotateCcwIcon, SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { RuleHelp, callKindOp, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { REPLAYS, useReplays } from "./api";
import { Said, State, when } from "./parts";
import { ReplayStart } from "./replay-start";
import { SectionStrip } from "./strip";
import type { ReplayItem } from "./types";
import { arnName, replaysSentence, windowWords } from "./words";

export function ReplaysList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; name?: string };
  if (search.create === "yes") return <ReplayStart archive={search.name} />;
  return <Replays service={service} kind={kind} />;
}

const going = (r: ReplayItem) => r.State === "STARTING" || r.State === "RUNNING";

function Replays({ service, kind }: { service: string; kind: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs(service);
  const q = useReplays();
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const all = useMemo(() => q.data?.Replays ?? [], [q.data]);
  const rows = useMemo(() => all.filter((r) => r.ReplayName.startsWith(typed.trim())), [all, typed]);
  const picked = all.filter((r) => chosen.has(r.ReplayName));
  const go = (search: { create?: "yes" }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search });
  const cancel = async () => {
    setError(null);
    try {
      for (const r of picked.filter(going)) await callKindOp(REPLAYS, "CancelReplay", { ReplayName: r.ReplayName });
      setChosen(new Set());
      await q.refetch();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const columns: DataColumn<ReplayItem>[] = [
    { id: "name", header: "Name", size: 230, min: 190, accessorFn: (r) => r.ReplayName, cell: (c) => <ResourceTo to={{ service, kind, name: c.row.original.ReplayName }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.ReplayName}</ResourceTo> },
    { id: "source", header: "Source", size: 130, accessorFn: (r) => arnName(r.EventSourceArn), cell: (c) => <ResourceTo to={{ service, kind: "archives", name: arnName(c.row.original.EventSourceArn) }} className="font-mono text-[13px]">{arnName(c.row.original.EventSourceArn)}</ResourceTo> },
    { id: "state", header: "State", size: 120, accessorFn: (r) => r.State ?? "", cell: (c) => <State state={c.row.original.State} /> },
    { id: "window", header: "Event window (UTC)", size: 300, min: 270, accessorFn: (r) => r.EventStartTime ?? 0, cell: (c) => windowWords(c.row.original.EventStartTime, c.row.original.EventEndTime) },
    { id: "last", header: "Last event replayed", size: 222, min: 222, accessorFn: (r) => r.EventLastReplayedTime ?? 0, cell: (c) => when(c.row.original.EventLastReplayedTime) },
    { id: "started", header: "Started", size: 222, min: 222, accessorFn: (r) => r.ReplayStartTime ?? 0, cell: (c) => when(c.row.original.ReplayStartTime) },
  ];
  return (
    <>
      <PageHeader
        title="Replays"
        crumbs={[...crumbs]}
        belowCrumbs={<SectionStrip current="replays" />}
        sub={<><b className="font-semibold text-foreground">Archived events sent back to the bus they came from.</b> Matching rules receive them again, as if they were new.</>}
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} interval={false} />}
        actions={<Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Start new replay</Button>}
      />
      {q.isError ? (
        <ErrorBlock what="The replays could not be listed" error={q.error} hint={HINT} onRetry={() => void q.refetch()} retrying={q.isFetching} testId="list-error" />
      ) : !q.data ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : all.length === 0 ? (
        <div data-testid="list-empty">
          <EmptyState icon={RotateCcwIcon} title="No replays" action={<Button variant="outline" className="bg-card" onClick={() => go({ create: "yes" })}><PlusIcon /> Start replay</Button>}>A replay sends archived events back to a bus. Start one from an archive.</EmptyState>
        </div>
      ) : (
        <>
          <Said text={replaysSentence(all)} names={all.map((r) => r.ReplayName)} bold={[/\b\d+(?= replays?\b)/]} />
          {error && <p className="mb-3 text-[13px] text-bad" role="alert">{error}</p>}
          <SelectableTable<ReplayItem>
            aria-label="Replays"
            data={rows}
            columns={columns}
            rowId={(r) => r.ReplayName}
            selected={chosen}
            onSelectedChange={setChosen}
            noun="replay"
            maxHeight={680}
            actionsAlways
            hint=" "
            toolbar={
              <form className="flex flex-wrap items-center gap-3" role="search" onSubmit={(e) => e.preventDefault()}>
                <div className="relative w-[22rem] max-w-full"><SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" /><Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search by name prefix" aria-label="Search by name prefix" className="bg-card pl-9" data-testid="list-search" /></div>
                <Button type="submit" variant="outline" className="bg-card">Search</Button>
              </form>
            }
            actions={(sel) => (
              <>
                {sel.length > 0 && !picked.some(going) && <span className="text-[13px] text-muted-foreground" data-testid="cancel-why">Only a replay that is Starting or Running can be cancelled.</span>}
                <Button type="button" variant="outline" className="bg-card" disabled={sel.length === 0 || !picked.some(going)} onClick={() => void cancel()} data-testid="action-cancel">Cancel replay</Button>
              </>
            )}
            empty={`No replay's name begins with “${typed}”.`}
          />
          <Pagination className="mt-3" shown={rows.length} total={rows.length} noun={rows.length === 1 ? "replay" : "replays"} />
          <p className="mt-4 max-w-3xl text-[13px] text-muted-foreground">Columns follow the fields ListReplays returns: ReplayName, EventSourceArn, State, EventStartTime and EventEndTime, EventLastReplayedTime, ReplayStartTime.</p>
          <RuleHelp className="mt-4 max-w-3xl">A replay runs in the background, in batches of events, and can be cancelled while it is <b>Starting</b> or <b>Running</b>; the pace is doze-aws's own. <b>Last event replayed</b> is the EventLastReplayedTime that DescribeReplay returns. Replayed events carry a <span className="font-mono">replay-name</span> field.</RuleHelp>
        </>
      )}
    </>
  );
}
