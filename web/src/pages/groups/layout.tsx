// One group (boards group-detail, group-detail-848, group-rebalancing and the tabs under them): the name once as the h1 with
// its state and pills, the actions, the figures as a stat strip, the chips that switch group, the tabs as routes (Members,
// Lag, Story, Wire), and the group's write dialogs, each routed by `?dialog=`.
import { useIsFetching, useQueryClient } from "@tanstack/react-query";
import { getRouteApi, Link, Outlet, useLocation, useNavigate } from "@tanstack/react-router";
import { RotateCcwIcon, Trash2Icon, UsersIcon } from "lucide-react";
import { ApiFailure } from "@/api/client";
import { groupKeys, useGroup } from "@/api/groups";
import type { GroupHead } from "@/api/types/GroupHead";
import { Combobox } from "@/components/app/combobox";
import { EmptyState } from "@/components/app/empty-state";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { type Stat, StatStrip } from "@/components/app/stat";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { cn } from "@/lib/utils";
import { PageError } from "../page-error";
import { GroupDialogs } from "./dialogs";
import { useRoutedOverlay } from "./overlay";
import { PhaseWord } from "./parts";
import { stamp } from "./words";

const route = getRouteApi("/groups/$group");

function pills(h: GroupHead): string[] {
  if (h.protocol === "consumer") return ["consumer protocol (KIP-848)", `epoch ${h.generation}`];
  const out = ["classic"];
  if (h.assignor && h.phase !== "empty") out.push(h.assignor);
  out.push(h.phase === "empty" ? "no members" : h.phase === "rebalancing" ? `generation ${h.generation + 1} forming` : `generation ${h.generation}`);
  return out;
}

function stats(h: GroupHead): Stat[] {
  return [
    { label: "State", value: h.phase[0]!.toUpperCase() + h.phase.slice(1), ch: 11 },
    { label: "Members", value: number(h.members.length), ch: 4 },
    { label: h.protocol === "consumer" ? "Epoch" : "Generation", value: String(h.generation), ch: 6, mono: true },
    { label: "Lag", value: number(h.lag_total), unit: "records behind", ch: 9 },
    { label: "Committed", value: number(h.commits), unit: h.commits === 1 ? "partition" : "partitions", ch: 4 },
    h.last_commit_ms !== null ? { label: "Last commit", value: stamp(h.last_commit_ms, h.now_ms), unit: "UTC", mono: true, ch: 11 } : { label: "Last commit", value: "never", mono: true, ch: 11 },
  ];
}

const TABS = [
  { to: "/groups/$group", label: "Members", tail: "", count: (h: GroupHead) => h.members.length },
  { to: "/groups/$group/lag", label: "Lag", tail: "/lag", count: (h: GroupHead) => h.lag_total },
  { to: "/groups/$group/story", label: "Story", tail: "/story", count: (h: GroupHead) => h.story.count },
  { to: "/groups/$group/wire", label: "Wire", tail: "/wire", count: undefined },
] as const;

/** Which tab the address is on (by the part after the group's segment). */
function useTab(): (typeof TABS)[number] {
  const path = useLocation({ select: (l) => l.pathname });
  return TABS.find((t) => t.tail && path.endsWith(t.tail)) ?? TABS[0];
}

function Switch({ h, tail }: { h: GroupHead; tail: string }) {
  const navigate = useNavigate();
  const to = `/groups/$group${tail}` as "/groups/$group";
  if (h.groups.length <= 8) {
    return (
      <div className="mb-6 flex flex-wrap items-center gap-2" data-testid="switch-group">
        <span className="mr-1 flex items-center gap-1.5 text-[13px] text-muted-foreground">
          <UsersIcon className="size-4" aria-hidden /> Switch group
        </span>
        {h.groups.map((g) =>
          g === h.id ? (
            <span key={g} aria-current="true" className="rounded-full border border-primary/30 bg-accent px-3.5 py-1 font-mono text-[12.5px] text-bright">
              {g}
            </span>
          ) : (
            <Link key={g} to={to} params={{ group: g }} className="rounded-full border bg-card px-3.5 py-1 font-mono text-[12.5px] text-foreground hover:border-primary/40 hover:text-primary focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
              {g}
            </Link>
          ),
        )}
      </div>
    );
  }
  return (
    <div className="mb-6 flex items-center gap-3" data-testid="switch-group">
      <span className="flex items-center gap-1.5 text-[13px] text-muted-foreground">
        <UsersIcon className="size-4" aria-hidden /> Switch group
      </span>
      <Combobox options={h.groups.map((g) => ({ value: g, label: g }))} value={h.id} onChange={(g) => g && g !== h.id && void navigate({ to, params: { group: g } })} search="Search groups…" aria-label="Switch group" className="w-72 font-mono" />
    </div>
  );
}

function Tabs({ h }: { h: GroupHead }) {
  const on = useTab();
  return (
    <nav aria-label="Group" className="mb-8 flex gap-1 border-b" data-testid="group-tabs">
      {TABS.map((t) => {
        const here = t === on;
        return (
          <Link
            key={t.label}
            to={t.to}
            params={{ group: h.id }}
            aria-current={here ? "page" : undefined}
            className={cn(
              "-mb-px flex items-center gap-2 border-b-2 px-3 pb-2.5 text-[14.5px] focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none",
              here ? "border-primary font-medium text-bright" : "border-transparent text-muted-foreground hover:text-foreground",
            )}
          >
            {t.label}
            {t.count && <span className="min-w-[2ch] rounded-md bg-well px-1.5 text-center font-mono text-[11.5px] text-muted-foreground tabular-nums">{number(t.count(h))}</span>}
          </Link>
        );
      })}
    </nav>
  );
}

export function NoGroup({ id, message }: { id: string; message: string }) {
  return (
    <>
      <PageHeader title="No such group" crumbs={[{ label: "Groups", to: "/groups" }]} />
      <EmptyState
        icon={UsersIcon}
        title={id}
        action={
          <Button asChild>
            <Link to="/groups">Back to Groups</Link>
          </Button>
        }
      >
        {message}
      </EmptyState>
    </>
  );
}

export function GroupLayout() {
  const { group: id } = route.useParams();
  const search = route.useSearch();
  const res = useGroup(id);
  const overlay = useRoutedOverlay("dialog");
  const qc = useQueryClient();
  const fetching = useIsFetching({ queryKey: groupKeys.group(id) }) > 0;
  const tab = useTab();
  const openDialog = (dialog: "reset" | "delete" | "delete-offsets") => overlay.open(dialog);
  if (res.isError && !res.data) {
    if (res.error instanceof ApiFailure && res.error.status === 404) return <NoGroup id={id} message={res.error.message} />;
    return <PageError error={res.error} onRetry={() => void res.refetch()} />;
  }
  const h = res.data?.head;
  const refresh = <RefreshControl updatedAt={res.dataUpdatedAt} fetching={fetching} onRefresh={() => void qc.invalidateQueries({ queryKey: groupKeys.group(id) })} />;
  return (
    <>
      <PageHeader
        title={id}
        mono
        crumbs={[{ label: "Groups", to: "/groups" }]}
        refresh={refresh}
        sub={
          h ? (
            <span className="flex flex-wrap items-center gap-2" data-testid="group-pills">
              <PhaseWord phase={h.phase} />
              {pills(h).map((p) => (
                <Badge key={p} variant="outline" className="rounded-full bg-card font-normal text-muted-foreground">
                  {p}
                </Badge>
              ))}
            </span>
          ) : (
            <Skeleton className="h-6 w-72" />
          )
        }
        actions={
          <>
            <Button variant="outline" className="bg-card" onClick={() => openDialog("reset")} data-testid="open-reset">
              <RotateCcwIcon /> Reset offsets
            </Button>
            <Button variant="outline" className="bg-card" onClick={() => openDialog("delete-offsets")} data-testid="open-delete-offsets">
              <Trash2Icon /> Delete offsets
            </Button>
            <Button variant="outline" className="border-bad/40 bg-card text-bad hover:bg-bad/5 hover:text-bad" onClick={() => openDialog("delete")} data-testid="open-delete">
              <Trash2Icon /> Delete group
            </Button>
          </>
        }
      />
      {h ? (
        <>
          <StatStrip stats={stats(h)} className="mb-6" />
          <Switch h={h} tail={tab.tail} />
          <Tabs h={h} />
          <Outlet />
          <GroupDialogs id={id} head={h} dialog={search.dialog} onClose={overlay.close} />
        </>
      ) : (
        <div className="flex flex-col gap-4" aria-busy>
          <Skeleton className="h-[106px] rounded-xl" />
          <Skeleton className="h-9 w-96 rounded-full" />
          <Skeleton className="h-80 rounded-xl" />
        </div>
      )}
    </>
  );
}
