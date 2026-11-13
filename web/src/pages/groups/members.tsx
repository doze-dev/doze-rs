// The Members tab, the group's own page (boards group-detail, group-members, group-detail-848, group-rebalancing): the state
// chooses the layout. A member opens as a Sheet over it (board member-detail), routed by `?member=`.
import { getRouteApi, Link } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { useGroup } from "@/api/groups";
import type { GroupHead } from "@/api/types/GroupHead";
import type { GroupLagView } from "@/api/types/GroupLagView";
import type { GroupMemberRow } from "@/api/types/GroupMemberRow";
import type { GroupPart } from "@/api/types/GroupPart";
import type { LagTopic } from "@/api/types/LagTopic";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { StatusDot } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { cn } from "@/lib/utils";
import { useRoutedOverlay } from "./overlay";
import { Facts, LagBar, Panel, Rules } from "./parts";
import { WireDock } from "@/pages/live/wire-dock";
import { agoNear, heard, memberLabel, numberWord, partList, PHASE_WORD, seconds, spanPrecise, stamp } from "./words";

const route = getRouteApi("/groups/$group/");

function role(h: GroupHead, m: GroupMemberRow): string {
  if (h.protocol === "consumer") return "Member";
  return m.leader ? "Leader" : "Follower";
}

function share(part: number, of: number): number {
  return part <= 0 || of <= 0 ? 0 : Math.min(100, Math.max(2, Math.floor((part * 100) / of)));
}

type Open = (id: string, from: HTMLElement) => void;

function MemberLink({ m, onOpen }: { m: GroupMemberRow; onOpen: Open }) {
  return (
    <button type="button" onClick={(e) => onOpen(m.id, e.currentTarget)} className="max-w-full truncate rounded-sm text-left font-mono text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" title={m.id} data-focus-key={`member:${m.id}`} data-testid="member-link">
      {memberLabel(m.id)}
    </button>
  );
}

const WIDE = "(min-width: 1280px)";
function subscribeWide(on: () => void) {
  const m = window.matchMedia(WIDE);
  m.addEventListener("change", on);
  return () => m.removeEventListener("change", on);
}
/** Whether the window is 1280 px or wider: below it the members table leaves Host and Session to the member's Sheet. */
function useWide(): boolean {
  return useSyncExternalStore(subscribeWide, () => window.matchMedia(WIDE).matches, () => true);
}

function MembersTable({ h, onOpen }: { h: GroupHead; onOpen: Open }) {
  const wide = useWide();
  // The member column depends on nothing that a refresh changes, so its cells (and the focus in them) stay.
  const memberCol = useMemo<DataColumn<GroupMemberRow>>(() => ({ id: "member", header: "Member id", accessorFn: (m) => m.id, cell: ({ row: { original: m } }) => <MemberLink m={m} onOpen={onOpen} /> }), [onOpen]);
  const cols = useMemo<DataColumn<GroupMemberRow>[]>(
    () => [
      memberCol,
      { id: "client", header: "Client id", size: 140, accessorFn: (m) => m.client_id, meta: { mono: true } },
      ...(wide ? [{ id: "host", header: "Host", size: 110, accessorFn: (m: GroupMemberRow) => m.host, meta: { mono: true } }] : []),
      { id: "role", header: "Role", size: 92, accessorFn: (m) => role(h, m) },
      { id: "generation", header: h.protocol === "consumer" ? "Epoch" : "Generation", size: 116, accessorFn: (m) => m.epoch, meta: { numeric: true, mono: true } },
      ...(wide ? [{ id: "session", header: "Session", size: 96, accessorFn: (m: GroupMemberRow) => (m.session_ms > 0 ? seconds(m.session_ms) : "—"), meta: { mono: true } }] : []),
      { id: "heard", header: "Last heartbeat", size: 144, accessorFn: (m) => heard(h.now_ms, m), meta: { mono: true } },
    ],
    [h, memberCol, wide],
  );
  return <DataTable data={h.members} columns={cols} rowId={(m) => m.id} aria-label="Members" empty="Nobody is in this group. Its committed offsets stay until the retention date." maxHeight={420} />;
}

function committedTo(t: LagTopic): string {
  const n = t.rows.filter((r) => r.committed !== null).length;
  return `${n} of ${t.partitions} ${t.partitions === 1 ? "partition" : "partitions"}`;
}

function ReadsTable({ topics }: { topics: LagTopic[] }) {
  const most = Math.max(0, ...topics.map((t) => t.behind));
  const cols: DataColumn<LagTopic>[] = [
    {
      id: "topic",
      header: "Topic",
      accessorFn: (t) => t.name,
      cell: ({ row: { original: t } }) => (
        <Link to="/topics/$topic" params={{ topic: t.name }} className="truncate font-mono text-link hover:underline">
          {t.name}
        </Link>
      ),
    },
    { id: "partitions", header: "Partitions", size: 116, accessorFn: (t) => t.partitions, meta: { numeric: true, mono: true } },
    { id: "committed", header: "Committed to", size: 156, accessorFn: committedTo },
    { id: "lag", header: "Lag", size: 220, accessorFn: (t) => t.behind, cell: ({ row: { original: t } }) => <LagBar pct={share(t.behind, most)} n={t.behind} /> },
  ];
  return <DataTable data={topics} columns={cols} rowId={(t) => t.name} aria-label="Reads" empty="The group has committed on no topic yet." maxHeight={360} />;
}

function TheGroup({ h }: { h: GroupHead }) {
  const session = Math.max(0, ...h.members.map((m) => m.session_ms));
  const rebalance = Math.max(0, ...h.members.map((m) => m.rebalance_ms ?? 0));
  const rows: [string, React.ReactNode][] = [
    ["Protocol type", <span className="font-mono">{h.protocol_type ?? "consumer"}</span>],
    ["Assignor", <span className="font-mono">{h.assignor ?? "none yet"}</span>],
    ["Generation", <span className="font-mono">{h.generation}</span>],
    ["Leader", <span className="font-mono">{h.leader ? memberLabel(h.leader) : "none yet"}</span>],
  ];
  if (session > 0) rows.push(["Session timeout", <span className="font-mono">{seconds(session)}</span>]);
  if (rebalance > 0) rows.push(["Rebalance timeout", <span className="font-mono">{seconds(rebalance)}</span>]);
  if (h.last_commit_ms !== null) rows.push(["Last commit", <span className="font-mono">{stamp(h.last_commit_ms, h.now_ms)}</span>]);
  return (
    <Panel title="The group" data-testid="the-group">
      <Facts rows={rows} />
      <p className="mt-4 text-[12.5px] text-muted-foreground">Timeouts are what the member asked for when it joined. The leader computes the assignment; the broker only hands it out.</p>
    </Panel>
  );
}

function WhoHolds({ v }: { v: GroupLagView }) {
  const h = v.head;
  const read = v.topics.filter((t) => t.exists && h.members.some((m) => m.holds.some((p) => p.topic === t.name)));
  if (read.length === 0) return null;
  const total = h.members.reduce((a, m) => a + m.holds.length, 0);
  const holderOf = (t: string, p: number) => h.members.find((m) => m.holds.some((x) => x.topic === t && x.partition === p));
  return (
    <Panel title="Who holds which partition" data-testid="who-holds">
      <p className="mb-4 text-[13px] text-muted-foreground">
        The assignment {h.protocol === "classic" ? `the leader computed at generation ${h.generation}` : `the broker targeted at epoch ${h.generation}`}. Each box is a partition of a topic this group reads.
      </p>
      <div className="flex flex-col gap-3">
        {read.map((t) => {
          const parts = Array.from({ length: t.partitions }, (_, i) => i);
          const owners = [...new Set(parts.map((p) => holderOf(t.name, p)?.id).filter(Boolean))] as string[];
          const allOne = owners.length === 1 && parts.every((p) => holderOf(t.name, p));
          return (
            <div key={t.name} className="flex flex-wrap items-center gap-2">
              <Link to="/topics/$topic" params={{ topic: t.name }} className="w-40 truncate font-mono text-[13px] text-link hover:underline">
                {t.name}
              </Link>
              {parts.map((p) => (
                <span key={p} className="rounded border bg-well px-1.5 font-mono text-[11.5px]" title={holderOf(t.name, p)?.id ?? "nobody"}>
                  {p}
                </span>
              ))}
              <span className="min-w-0 basis-full text-[12.5px] break-all text-muted-foreground">{allOne ? `all ${numberWord(parts.length)}: ${memberLabel(owners[0]!)}` : parts.map((p) => `${p}: ${holderOf(t.name, p) ? memberLabel(holderOf(t.name, p)!.id) : "nobody"}`).join(", ")}</span>
            </div>
          );
        })}
      </div>
      <p className="mt-4 border-t pt-3 text-[12.5px] text-muted-foreground">
        {h.members.length === 1 ? (
          <>
            This is one process reading {numberWord(total)} partitions. Lag grows when a single consumer cannot keep up; a second instance with the same <code className="font-mono">group.id</code> would split these partitions at the next rebalance.
          </>
        ) : (
          `${h.members.length} members share these partitions. A member that leaves hands its partitions to the others at the next rebalance.`
        )}
      </p>
    </Panel>
  );
}

function Classic({ v, onOpen }: { v: GroupLagView; onOpen: Open }) {
  const h = v.head;
  return (
    <div className="flex flex-col gap-8">
      <section>
        <h2 className="mb-3 text-[17px] font-semibold text-bright">Members</h2>
        <MembersTable h={h} onOpen={onOpen} />
      </section>
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_24rem]">
        <div className="flex min-w-0 flex-col gap-8">
          <section>
            <h2 className="mb-3 text-[17px] font-semibold text-bright">Reads</h2>
            <ReadsTable topics={v.topics} />
          </section>
          <WhoHolds v={v} />
        </div>
        <div className="min-w-0">
          <TheGroup h={h} />
        </div>
      </div>
    </div>
  );
}

function inStep(m: GroupMemberRow, target: number): boolean {
  const same = (a: GroupPart[], b: GroupPart[]) => a.length === b.length && a.every((p, i) => p.topic === b[i]!.topic && p.partition === b[i]!.partition);
  return same(m.holds, m.target) && m.epoch === target;
}

function Consumer({ v, onOpen }: { v: GroupLagView; onOpen: Open }) {
  const h = v.head;
  const target = h.target_epoch ?? h.generation;
  const big = (label: string, value: React.ReactNode, help: React.ReactNode) => (
    <div className="min-w-0 rounded-xl border bg-card p-5">
      <div className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">{label}</div>
      <div className="mt-1 truncate font-mono text-[28px] leading-10 text-bright tabular-nums">{value}</div>
      <p className="mt-1 text-[12.5px] text-muted-foreground">{help}</p>
    </div>
  );
  const cols: DataColumn<GroupMemberRow>[] = [
    { id: "member", header: "Member", accessorFn: (m) => m.id, cell: ({ row: { original: m } }) => <MemberLink m={m} onOpen={onOpen} /> },
    { id: "client", header: "Client id", size: 130, accessorFn: (m) => m.client_id, meta: { mono: true } },
    { id: "epoch", header: "Member epoch", size: 120, accessorFn: (m) => m.epoch, meta: { numeric: true, mono: true } },
    { id: "target", header: "Target", accessorFn: (m) => partList(m.target), meta: { mono: true } },
    { id: "assigned", header: "Assigned", accessorFn: (m) => partList(m.holds), meta: { mono: true } },
    {
      id: "status",
      header: "Status",
      size: 120,
      accessorFn: (m) => (inStep(m, target) ? "In step" : "Reconciling"),
      cell: ({ row: { original: m } }) => (inStep(m, target) ? <StatusDot tone="ok">In step</StatusDot> : <StatusDot tone="warn">Reconciling</StatusDot>),
    },
  ];
  const where: [string, string, string][] = [
    ["empty", "Empty", "No member. Offsets are kept until retention."],
    ["assigning", "Assigning", "The epoch moved; targets are computed at the next heartbeat, at least 1 s after the last time."],
    ["reconciling", "Reconciling", "Targets differ from what members hold. A member hands a partition back before another takes it."],
    ["stable", "Stable", "Every member holds its target."],
  ];
  return (
    <div className="flex flex-col gap-8" data-testid="consumer-group">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {big("Group epoch", h.generation, "Moves whenever membership, subscriptions or topics change.")}
        {big("Targets computed for", target, target === h.generation ? "The same epoch, so every member has its target." : "An older epoch: the targets are computed at the next heartbeat.")}
        {big("Assignor", h.assignor ?? "uniform", "The broker assigns. Clients do not elect a leader.")}
        {big("Session timeout", h.session_ms !== null ? seconds(h.session_ms) : "—", <>From <code className="font-mono">--consumer-session-timeout</code>.</>)}
      </div>
      <section>
        <h2 className="text-[17px] font-semibold text-bright">Members, target and assigned</h2>
        <p className="mt-1 mb-3 max-w-[52rem] text-[13.5px] text-muted-foreground">Target is what the broker wants this member to hold at the group epoch. Assigned is what it holds now. When they differ the group is Reconciling.</p>
        <DataTable data={h.members} columns={cols} rowId={(m) => m.id} aria-label="Members" empty="Nobody is in this group." maxHeight={420} />
      </section>
      <div className="grid gap-6 xl:grid-cols-2">
        <Panel title="Where a group can be">
          <Rules rows={where.map(([p, w, sub]) => ({ ok: h.phase === p, here: h.phase === p, title: w, sub: h.phase === p ? `${sub} This group, now.` : sub, value: h.phase === p ? "now" : "" }))} />
        </Panel>
        <Panel title="Reads">
          <ReadsTable topics={v.topics} />
          <p className="mt-3 text-[12.5px] text-muted-foreground">Offsets are committed with the member epoch. A commit at an old epoch is refused with STALE_MEMBER_EPOCH.</p>
        </Panel>
      </div>
    </div>
  );
}

/** The time left to a deadline, counted down here each tenth of a second with no request (live-design.md, CON-812). */
export function Countdown({ deadline, className }: { deadline: number; className?: string }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(t);
  }, []);
  const left = Math.max(0, deadline - now);
  return (
    <time dateTime={new Date(deadline).toISOString()} className={cn("inline-block min-w-[5ch] text-right font-mono tabular-nums", className)} data-testid="countdown">
      {spanPrecise(left)}
    </time>
  );
}

function Rebalancing({ v, fetchedAt, onOpen }: { v: GroupLagView; fetchedAt: number; onOpen: Open }) {
  const h = v.head;
  const r = h.rebalance;
  const waiting = r?.waiting_for[0];
  const deadline = fetchedAt + (r?.remaining_ms ?? 0);
  const pct = r && r.timeout_ms > 0 ? 100 - Math.min(100, Math.floor((r.remaining_ms * 100) / r.timeout_ms)) : 100;
  const cols: DataColumn<GroupMemberRow>[] = [
    { id: "member", header: "Member", accessorFn: (m) => m.id, cell: ({ row: { original: m } }) => <MemberLink m={m} onOpen={onOpen} /> },
    { id: "state", header: "State", size: 150, accessorFn: (m) => (m.rejoined ? "Rejoined" : "Not rejoined"), cell: ({ row: { original: m } }) => (m.rejoined ? <StatusDot tone="ok">Rejoined</StatusDot> : <StatusDot tone="warn">Not rejoined</StatusDot>) },
    { id: "held", header: `Held at generation ${h.generation}`, accessorFn: (m) => partList(m.holds), meta: { mono: true } },
    { id: "heard", header: "Last heard", size: 150, accessorFn: (m) => heard(h.now_ms, m), meta: { mono: true } },
  ];
  const left = h.story.left;
  return (
    <div className="flex flex-col gap-8" data-testid="rebalancing-group">
      <div className="grid gap-6 xl:grid-cols-2">
        <Panel>
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="min-w-0">
              <div className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">Waiting for</div>
              <div className="mt-1 truncate font-mono text-[22px] leading-[30px] text-bright" title={waiting}>
                {waiting ? memberLabel(waiting) : "the leader's assignment"}
              </div>
            </div>
            <div className="text-right text-[13px] text-muted-foreground">
              <Countdown deadline={deadline} className="text-[24px] leading-9 text-bright" /> <span>of {r ? spanPrecise(r.timeout_ms) : "–"} left</span>
            </div>
          </div>
          <div className="mt-5 h-1.5 overflow-hidden rounded-full bg-track" aria-hidden>
            <div className="h-full rounded-full bg-warn" style={{ width: `${pct}%` }} />
          </div>
          <p className="mt-4 text-[12.5px] text-muted-foreground">Members that have not joined again by the deadline are dropped.</p>
        </Panel>
        <Panel title="What happens next">
          <Rules
            rows={[
              { ok: true, title: `${waiting ? memberLabel(waiting) : "The members"} rejoins before the deadline`, sub: `Generation ${h.generation + 1} forms with the members that joined, and the leader assigns the partitions.` },
              { blocking: true, title: "It does not", sub: `${waiting ? memberLabel(waiting) : "The members"} is dropped and, if it was the last, the group goes Empty. Its committed offsets stay.` },
            ]}
          />
        </Panel>
      </div>
      <section>
        <h2 className="mb-3 text-[17px] font-semibold text-bright">Members</h2>
        <DataTable data={h.members} columns={cols} rowId={(m) => m.id} aria-label="Members" maxHeight={420} />
        {left && (
          <p className="mt-3 text-[13px] text-muted-foreground" data-testid="member-left">
            <code className="font-mono text-foreground" title={left.member}>
              {memberLabel(left.member)}
            </code>{" "}
            {left.cause === "member_left" ? "sent LeaveGroup" : "timed out"} {agoNear(left.ago_ms)} and is no longer in the group; it held {partList(left.held.flatMap(([t, ps]) => ps.map((p) => ({ topic: t, partition: p })))) || "nothing"}.
          </p>
        )}
      </section>
    </div>
  );
}

function MemberSheet({ h, member, onClose, returnFocus }: { h: GroupHead; member?: string; onClose: () => void; returnFocus: (e: Event) => void }) {
  const m = member ? h.members.find((x) => x.id === member) : undefined;
  const firstTopic = m?.holds[0]?.topic;
  return (
    <Sheet open={!!member} onOpenChange={(o) => !o && onClose()}>
      <SheetContent onCloseAutoFocus={returnFocus} className="w-full sm:max-w-xl" data-testid="member-sheet">
        <SheetHeader>
          <SheetTitle className="font-mono break-all">{m ? memberLabel(m.id) : member}</SheetTitle>
          <SheetDescription>
            {m ? (
              <>
                A member of <span className="font-mono">{h.id}</span> {h.protocol === "classic" ? `at generation ${m.epoch}` : `at member epoch ${m.epoch}`}. {PHASE_WORD[h.phase]}.
              </>
            ) : (
              "This member is not in the group any more."
            )}
          </SheetDescription>
        </SheetHeader>
        {m && (
          <div className="flex flex-col gap-6 px-4 pb-6">
            <Facts
              rows={[
                ["Member id", <span className="font-mono text-[12.5px] break-all">{m.id}</span>],
                ["Client id", <span className="font-mono">{m.client_id}</span>],
                ["Host", <span className="font-mono">{m.host}</span>],
                ["Role", h.protocol === "consumer" ? "The broker assigns; a member does not lead" : m.leader ? `Leader: computed the ${h.assignor ?? ""} assignment` : "Follower: takes the assignment the leader computed"],
                ["Generation", <span className="font-mono">{m.epoch}</span>],
                [
                  "Session timeout",
                  <span className="flex flex-col">
                    <span className="font-mono">{m.session_ms > 0 ? seconds(m.session_ms) : "—"}</span>
                    <span className="text-[12px] text-muted-foreground">{heard(h.now_ms, m)} is when it last sent a heartbeat</span>
                  </span>,
                ],
                ["Rebalance timeout", <span className="font-mono">{m.rebalance_ms !== null ? seconds(m.rebalance_ms) : "not sent"}</span>],
                ["Static instance id", m.instance ? <span className="font-mono">{m.instance}</span> : <span className="text-muted-foreground">none: a restart makes a new member</span>],
                [
                  "Holds",
                  m.holds.length ? (
                    <span className="flex flex-col font-mono" data-testid="member-holds">
                      {partList(m.holds)
                        .split(", ")
                        .map((p) => (
                          <span key={p}>{p}</span>
                        ))}
                    </span>
                  ) : (
                    "nothing"
                  ),
                ],
              ]}
            />
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" asChild>
                <Link to="/live" search={{ client: [m.client_id] }}>
                  Its requests in Live
                </Link>
              </Button>
              {firstTopic && (
                <Button variant="outline" asChild>
                  <Link to="/topics/$topic" params={{ topic: firstTopic }}>
                    Open {firstTopic}
                  </Link>
                </Button>
              )}
              <Button variant="ghost" onClick={onClose}>
                Close
              </Button>
            </div>
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}

export function GroupMembersPage() {
  const { group: id } = route.useParams();
  const search = route.useSearch();
  const res = useGroup(id);
  const overlay = useRoutedOverlay("member");
  const open = useCallback<Open>((m, from) => overlay.open(m, from), [overlay]);
  const v = res.data;
  if (!v) return null;
  const h = v.head;
  return (
    <>
      {h.protocol === "consumer" ? <Consumer v={v} onOpen={open} /> : h.phase === "rebalancing" ? <Rebalancing v={v} fetchedAt={res.dataUpdatedAt} onOpen={open} /> : <Classic v={v} onOpen={open} />}
      <section className="mt-12">
        <h2 className="mb-3 text-[17px] font-semibold text-bright">What just happened</h2>
        <WireDock group={id} />
      </section>
      <MemberSheet h={h} member={search.member} onClose={overlay.close} returnFocus={overlay.returnFocus} />
    </>
  );
}
