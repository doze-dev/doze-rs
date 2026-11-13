// The Groups list (boards groups-list, groups-none): the counts as stat tiles, the line about a group that is rebalancing in a
// region of reserved height, the name filter and the state MultiSelect in the address, and the DataTable.
import { getRouteApi, Link } from "@tanstack/react-router";
import { SearchIcon, UsersIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useGroups } from "@/api/groups";
import type { GroupRow } from "@/api/types/GroupRow";
import type { GroupsResponse } from "@/api/types/GroupsResponse";
import type { RowLag } from "@/api/types/RowLag";
import { CodeValue } from "@/components/app/code-value";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { EmptyState } from "@/components/app/empty-state";
import { MultiSelect } from "@/components/app/multi-select";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { StatStrip } from "@/components/app/stat";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { PageError } from "../page-error";
import { Callout, GroupSpans, PhaseWord } from "./parts";
import { type GroupState, STATES } from "./search";
import { stamp } from "./words";

const route = getRouteApi("/groups");

function protocolWord(r: GroupRow): string {
  if (r.protocol === "consumer") return "consumer (KIP-848)";
  return r.assignor && r.phase !== "empty" ? `classic, ${r.assignor}` : "classic";
}

function nameSub(r: GroupRow, now: number): string {
  if (r.phase === "rebalancing" && r.protocol === "classic") return `generation ${r.generation + 1} is forming`;
  if (r.phase === "empty" && r.expires_ms !== null) return `offsets expire ${stamp(r.expires_ms, now)}`;
  const last = r.story.last;
  if (r.protocol === "consumer" && last && !last.forming) return `last rebalance ${stamp(now - last.ago_ms, now)}`;
  if (r.last_commit_ms !== null) return `last commit ${stamp(r.last_commit_ms, now)}`;
  return r.lag.any_commit ? "" : "no commits yet";
}

function stateSub(r: GroupRow): string {
  if (r.protocol === "consumer") return `epoch ${r.generation}`;
  return r.phase === "stable" ? `generation ${r.generation}` : "";
}

function LagCell({ l }: { l: RowLag }) {
  if (l.ahead) {
    return (
      <div className="flex min-w-0 flex-col">
        <span className="truncate text-[13px] font-semibold text-warn">Past the end</span>
        <span className="truncate text-[12px] text-muted-foreground">
          {l.ahead[0]} is {number(l.ahead[1])} beyond its log
        </span>
      </div>
    );
  }
  if (!l.any_commit) return <span className="text-muted-foreground">no commits</span>;
  const sub = l.each !== null ? `${number(l.each)} in each partition` : l.uncommitted[0] ? `${l.uncommitted[0]}: no commits yet` : "";
  return (
    <div className="flex min-w-0 flex-col">
      <span className="font-mono text-bright tabular-nums">{number(l.total)}</span>
      <span className="truncate text-[12px] text-muted-foreground">{sub || " "}</span>
    </div>
  );
}

function columns(now: number): DataColumn<GroupRow>[] {
  return [
    {
      id: "group",
      header: "Group",
      accessorFn: (r) => r.id,
      cell: ({ row: { original: r } }) => {
        const sub = nameSub(r, now);
        return (
          <div className="flex min-w-0 flex-col">
            <Link to="/groups/$group" params={{ group: r.id }} className="truncate font-mono text-link hover:underline" title={r.id} data-testid="group-link">
              {r.id}
            </Link>
            <span className="truncate text-[12px] text-muted-foreground" title={sub}>
              {sub || " "}
            </span>
          </div>
        );
      },
    },
    { id: "protocol", header: "Protocol", size: 176, accessorFn: protocolWord },
    {
      id: "state",
      header: "State",
      size: 150,
      accessorFn: (r) => r.phase,
      cell: ({ row: { original: r } }) => (
        <div className="flex min-w-0 flex-col">
          <PhaseWord phase={r.phase} />
          <span className="truncate text-[12px] text-muted-foreground">{stateSub(r) || " "}</span>
        </div>
      ),
    },
    { id: "members", header: "Members", size: 104, accessorFn: (r) => r.members, meta: { numeric: true, mono: true } },
    { id: "reads", header: "Reads", size: 200, accessorFn: (r) => r.reads.join(", "), meta: { mono: true } },
    { id: "lag", header: "Lag", size: 200, accessorFn: (r) => r.lag.total, cell: ({ row: { original: r } }) => <LagCell l={r.lag} /> },
  ];
}

function NoGroups({ r }: { r: GroupsResponse }) {
  return (
    <>
      <EmptyState
        icon={UsersIcon}
        title="Groups are made by consumers, not here"
        action={
          <Button asChild>
            <Link to="/topics">Go to Topics</Link>
          </Button>
        }
      >
        A group exists once a consumer joins with a <code className="font-mono">group.id</code>, or an app commits an offset for one. Start a consumer and it shows up here with its members and lag in a few seconds.
        <CodeValue className="mt-4 text-left" value={`kcat -C -b ${r.listen} -G payments-workers orders`} label="Command" />
      </EmptyState>
      <p className="mt-5 text-[13px] text-muted-foreground">Nothing to refresh by hand. An empty group with no committed offsets is forgotten by the broker at its next sweep and leaves this list.</p>
    </>
  );
}

/** The line about a group that is rebalancing, in a region of reserved height: a calm line when none is. */
function Attention({ r }: { r: GroupsResponse }) {
  const a = r.view.attention;
  return (
    <div className="min-h-[76px]" data-testid="groups-attention">
      {a && r.attention ? (
        <Callout tone="warn">
          <GroupSpans spans={r.attention} />{" "}
          <Link to="/groups/$group" params={{ group: a.id }} className="font-medium text-link hover:underline">
            Open the group
          </Link>
          {a.last && (
            <>
              {" or "}
              <Link to="/groups/$group/story" params={{ group: a.id }} className="font-medium text-link hover:underline">
                read its story
              </Link>
            </>
          )}
          .
        </Callout>
      ) : (
        <Callout tone="ok">No group is rebalancing.</Callout>
      )}
    </div>
  );
}

export function GroupsPage() {
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const q = search.q ?? "";
  const states = useMemo(() => search.state ?? [], [search.state]);
  const res = useGroups(q, states);
  const [text, setText] = useState(q);
  // Typing filters as it goes: the address follows a moment later (replace, so Back leaves the page, not each letter).
  useEffect(() => {
    if (text.trim() === q) return;
    const t = setTimeout(() => void navigate({ search: (s) => ({ ...s, q: text.trim() || undefined }), replace: true, resetScroll: false }), 200);
    return () => clearTimeout(t);
  }, [text, q, navigate]);
  const setStates = (v: string[]) => {
    const next = STATES.filter((s) => v.includes(s)) as GroupState[];
    void navigate({ search: (s) => ({ ...s, state: next.length ? next : undefined }), replace: true, resetScroll: false });
  };
  const data = res.data;
  const cols = useMemo(() => columns(data?.view.now_ms ?? 0), [data?.view.now_ms]);
  const refresh = <RefreshControl updatedAt={res.dataUpdatedAt} fetching={res.isFetching} onRefresh={() => void res.refetch()} />;
  const header = (sub: string) => <PageHeader title={data && data.view.total === 0 ? "No groups yet" : "Groups"} crumbs={[{ label: "Home", to: "/" }]} sub={sub} refresh={refresh} />;
  if (res.isError && !data) return <PageError error={res.error} onRetry={() => void res.refetch()} />;
  if (!data) {
    return (
      <>
        {header("Consumer groups on this broker.")}
        <div className="flex flex-col gap-4" aria-busy>
          <Skeleton className="h-[106px] rounded-xl" />
          <Skeleton className="h-[76px] rounded-xl" />
          <Skeleton className="h-80 rounded-xl" />
        </div>
      </>
    );
  }
  const v = data.view;
  if (v.total === 0) {
    return (
      <>
        {header("No consumer group has joined or committed on this broker.")}
        <NoGroups r={data} />
      </>
    );
  }
  const counts = v.counts;
  const options = STATES.map((s, i) => ({ value: s, label: s[0]!.toUpperCase() + s.slice(1), hint: number(counts[i + 1] ?? 0) }));
  const shown = v.truncated ? `First ${number(v.rows.length)} of ${number(v.total)} groups, by id` : `${number(v.rows.length)} of ${number(v.total)} groups`;
  return (
    <>
      {header("Consumer groups on this broker. A group appears when a consumer joins with a group.id or commits an offset; there is nothing to create here.")}
      <StatStrip
        stats={[
          { label: "Groups", value: number(counts[0]), ch: 5 },
          { label: "Stable", value: number(counts[1]), ch: 5 },
          { label: "Rebalancing", value: number(counts[2]), ch: 5 },
          { label: "Empty", value: number(counts[3]), ch: 5 },
        ]}
      />
      <div className="mt-4">
        <Attention r={data} />
      </div>
      <div className="mt-4 mb-3 flex flex-wrap items-start gap-3" data-testid="groups-toolbar">
        <div className="relative w-72 max-w-full">
          <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
          <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="Filter by group id" aria-label="Filter by group id" className="bg-card pl-9" data-testid="groups-filter" />
        </div>
        <MultiSelect options={options} value={states} onChange={setStates} noun="states" aria-label="States" triggerClassName="w-52" />
        <span className="ml-auto self-center text-[13px] text-muted-foreground">Listed by id</span>
      </div>
      <DataTable
        data={v.rows}
        columns={cols}
        rowId={(r) => r.id}
        aria-label="Groups"
        maxHeight={720}
        className="[&_tbody_tr]:h-[60px] [&_tbody_td]:h-[60px]"
        empty={v.filter ? `No group id contains ${v.filter}.` : "No group is in this state."}
      />
      <div className="mt-3 flex flex-wrap items-center justify-between gap-2 text-[13px] text-muted-foreground">
        <span>Lag is the log end minus the committed offset, worked out here from OffsetFetch and ListOffsets. The broker keeps no lag of its own.</span>
        <span className="tabular-nums whitespace-nowrap" data-testid="groups-shown">
          {shown}
        </span>
      </div>
      <p className="mt-2 text-[12.5px] text-muted-foreground">Open a group's Lag tab for the partitions.</p>
    </>
  );
}
