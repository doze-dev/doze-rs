// The Members tab of a streams group (KIP-1071): a Kafka Streams application's members are its stream threads, and the
// broker hands them tasks, not partitions. Task p of a subtopology reads partition p of each topic the subtopology reads.
// The page says why the topology cannot run when it cannot (the status the members are told), the epochs, the topology
// (subtopologies and the internal topics made for them), each member's tasks by role (held, targeted, being given up)
// and the lag of what the group committed on the topics its tasks read.
import { Link } from "@tanstack/react-router";
import type { GroupLagView } from "@/api/types/GroupLagView";
import type { GroupMemberRow } from "@/api/types/GroupMemberRow";
import type { GroupStreamsView } from "@/api/types/GroupStreamsView";
import type { StreamsInternalTopic } from "@/api/types/StreamsInternalTopic";
import type { StreamsMemberRow } from "@/api/types/StreamsMemberRow";
import type { StreamsRoles } from "@/api/types/StreamsRoles";
import type { StreamsSubtopologyRow } from "@/api/types/StreamsSubtopologyRow";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { StatusDot } from "@/components/app/status-dot";
import { number } from "@/lib/format";
import { MemberLink, type Open, ReadsTable, useWide } from "./members";
import { Callout, Panel, Rules } from "./parts";
import { taskList } from "./words";

const same = (a: string[], b: string[]) => a.length === b.length && a.every((x) => b.includes(x));

/** What the members are told when the topology cannot run, and what to do about it, by status. */
const NOT_READY: Record<string, string> = {
  MISSING_SOURCE_TOPICS: "Create the topics the application reads (or start the producer that makes them); the group tries again at the next heartbeat.",
  MISSING_INTERNAL_TOPICS: "The broker makes the repartition and changelog topics itself; it is making them now, or was refused (topic auto-creation is not needed, but the console's principal and the broker's limits are).",
  INCORRECTLY_PARTITIONED_TOPICS: "Topics that are joined must have as many partitions as each other. Repartition one of them, or re-key it through a repartition topic.",
};

function Big({ label, value, help }: { label: string; value: React.ReactNode; help: React.ReactNode }) {
  return (
    <div className="min-w-0 rounded-xl border bg-card p-5">
      <div className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">{label}</div>
      <div className="mt-1 truncate font-mono text-[28px] leading-10 text-bright tabular-nums">{value}</div>
      <p className="mt-1 text-[12.5px] text-muted-foreground">{help}</p>
    </div>
  );
}

function TopicLinks({ names }: { names: string[] }) {
  if (names.length === 0) return <span className="text-muted-foreground">none</span>;
  return (
    <span className="flex min-w-0 flex-wrap gap-x-3 gap-y-0.5">
      {names.map((n) => (
        <Link key={n} to="/topics/$topic" params={{ topic: n }} className="min-w-0 truncate font-mono text-[12.5px] text-link hover:underline" title={n}>
          {n}
        </Link>
      ))}
    </span>
  );
}

function Subtopology({ s }: { s: StreamsSubtopologyRow }) {
  const rows: [string, string[]][] = [
    ["Reads", s.sources],
    ["Re-keys into", s.repartition_sinks],
    ["Reads re-keyed", s.repartition_sources],
    ["State changelogs", s.changelogs],
  ];
  return (
    <div className="min-w-0 rounded-xl border bg-card p-5" data-testid="streams-subtopology">
      <div className="mb-3 flex items-baseline justify-between gap-3">
        <h3 className="text-[15px] font-semibold text-bright">Subtopology {s.id}</h3>
        <span className="font-mono text-[12.5px] whitespace-nowrap text-muted-foreground tabular-nums">{s.tasks === null ? "tasks once ready" : `${number(s.tasks)} ${s.tasks === 1 ? "task" : "tasks"}`}</span>
      </div>
      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-5 gap-y-2 text-[13px]">
        {rows
          .filter(([, names]) => names.length > 0)
          .map(([k, names]) => (
            <div key={k} className="contents">
              <dt className="whitespace-nowrap text-muted-foreground">{k}</dt>
              <dd className="min-w-0">
                <TopicLinks names={names} />
              </dd>
            </div>
          ))}
      </dl>
    </div>
  );
}

function internalColumns(wide: boolean): DataColumn<StreamsInternalTopic>[] {
  const all: DataColumn<StreamsInternalTopic>[] = [
    {
      id: "name",
      header: "Topic",
      accessorFn: (t) => t.name,
      cell: ({ row: { original: t } }) =>
        t.exists !== null ? (
          <Link to="/topics/$topic" params={{ topic: t.name }} className="truncate font-mono text-link hover:underline">
            {t.name}
          </Link>
        ) : (
          <span className="truncate font-mono">{t.name}</span>
        ),
    },
    { id: "kind", header: "Kind", size: 112, accessorFn: (t) => t.kind },
    { id: "sub", header: "Subtopology", size: 112, accessorFn: (t) => t.subtopology, meta: { numeric: true, mono: true } },
    { id: "partitions", header: "Partitions", size: 116, accessorFn: (t) => (t.partitions === null ? "—" : number(t.partitions)), meta: { numeric: true, mono: true } },
    {
      id: "exists",
      header: "On the broker",
      size: 170,
      accessorFn: (t) => t.exists ?? -1,
      cell: ({ row: { original: t } }) =>
        t.exists === null ? (
          <StatusDot tone="warn">not made yet</StatusDot>
        ) : t.partitions !== null && t.exists !== t.partitions ? (
          <StatusDot tone="bad">{number(t.exists)} partitions</StatusDot>
        ) : (
          <StatusDot tone="ok">made</StatusDot>
        ),
    },
    { id: "configs", header: "Configs asked for", size: 200, accessorFn: (t) => t.configs.map(([k, v]) => `${k}=${v}`).join(", ") || "—", meta: { mono: true } },
  ];
  // Below 1280 px the configs are left out: the topic's name keeps its room.
  return wide ? all : all.filter((c) => c.id !== "configs");
}

/** One role's cell: the tasks held, then (when it differs) the target, and what is being given up. */
function RoleCell({ m, role }: { m: StreamsMemberRow; role: keyof StreamsRoles }) {
  const held = m.holds[role];
  const target = m.target[role];
  const revoking = m.revoking[role];
  return (
    <div className="flex min-w-0 flex-col">
      <span className="truncate font-mono text-[12.5px]" title={held.join(" ")}>
        {held.length === 0 ? "—" : taskList(held)}
      </span>
      {!same(held, target) && <span className="truncate text-[12px] text-warn">to run {target.length === 0 ? "none" : taskList(target)}</span>}
      {revoking.length > 0 && <span className="truncate text-[12px] text-warn">giving up {taskList(revoking)}</span>}
    </div>
  );
}

const STAGE: Record<string, [string, "ok" | "warn"]> = {
  STABLE: ["In step", "ok"],
  UNREVOKED_TASKS: ["Giving up tasks", "warn"],
  UNRELEASED_TASKS: ["Waiting for tasks", "warn"],
};

function memberColumns(rows: GroupMemberRow[], onOpen: Open, wide: boolean): DataColumn<StreamsMemberRow>[] {
  const link = (m: StreamsMemberRow) => {
    const row = rows.find((r) => r.id === m.id);
    return row ? <MemberLink m={row} onOpen={onOpen} /> : <span className="truncate font-mono">{m.id}</span>;
  };
  const all: DataColumn<StreamsMemberRow>[] = [
    { id: "member", header: "Member", accessorFn: (m) => m.id, cell: ({ row: { original: m } }) => link(m) },
    { id: "process", header: "Process", size: 140, accessorFn: (m) => m.process, meta: { mono: true } },
    { id: "endpoint", header: "Endpoint", size: 150, accessorFn: (m) => m.endpoint ?? "—", meta: { mono: true } },
    { id: "epoch", header: "Epoch", size: 84, accessorFn: (m) => m.epoch, meta: { numeric: true, mono: true } },
    { id: "active", header: "Active", size: 150, accessorFn: (m) => m.holds.active.length, cell: ({ row: { original: m } }) => <RoleCell m={m} role="active" /> },
    { id: "standby", header: "Standby", size: 100, accessorFn: (m) => m.holds.standby.length, cell: ({ row: { original: m } }) => <RoleCell m={m} role="standby" /> },
    { id: "warmup", header: "Warm-up", size: 100, accessorFn: (m) => m.holds.warmup.length, cell: ({ row: { original: m } }) => <RoleCell m={m} role="warmup" /> },
    {
      id: "stage",
      header: "Stage",
      size: 160,
      accessorFn: (m) => m.stage,
      cell: ({ row: { original: m } }) => {
        const [word, tone] = STAGE[m.stage] ?? [m.stage, "warn"];
        return (
          <StatusDot tone={tone} title={m.stage}>
            {word}
          </StatusDot>
        );
      },
    },
  ];
  // Below 1280 px the process and the endpoint are left out: the tasks keep their room.
  return wide ? all : all.filter((c) => c.id !== "process" && c.id !== "endpoint");
}

function NotReady({ s }: { s: GroupStreamsView }) {
  const n = s.not_ready;
  if (!n) return null;
  return (
    <Callout tone="warn" data-testid="streams-not-ready">
      <b className="font-semibold text-bright">The topology cannot run yet.</b> Every member is told <code className="font-mono text-[12.5px] text-foreground">{n.name}</code> at each heartbeat, with Kafka's words <span data-testid="streams-not-ready-words">"{n.detail}"</span> No task is
      handed out until it can run. {NOT_READY[n.name] ?? ""}
    </Callout>
  );
}

const WHERE: [string, string, string][] = [
  ["Empty", "Empty", "No member. The topology and the committed offsets are kept; offsets may be reset now."],
  ["NotReady", "Not ready", "Members are in, but a topic the topology needs is missing or partitioned unlike its partner: no task is given."],
  ["Assigning", "Assigning", "The group epoch moved; the tasks are worked out again at the next heartbeat (streams.assignment.interval.ms apart)."],
  ["Reconciling", "Reconciling", "A member is giving up tasks, or waiting for tasks another member has not yet given up."],
  ["Stable", "Stable", "Every member runs the tasks it is to run."],
];

/** A streams group's Members tab. */
export function StreamsMembers({ v, onOpen }: { v: GroupLagView; onOpen: Open }) {
  const s = v.streams;
  const h = v.head;
  const wide = useWide();
  if (!s) return null;
  const tasks = s.subtopologies.every((t) => t.tasks !== null) && s.subtopologies.length > 0 ? s.subtopologies.reduce((n, t) => n + (t.tasks ?? 0), 0) : null;
  return (
    <div className="flex flex-col gap-8" data-testid="streams-group">
      <NotReady s={s} />
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <Big label="Group epoch" value={s.epoch} help="Moves when a member joins or leaves, the topology changes, or a topic it reads grows." />
        <Big label="Assignment epoch" value={s.assignment_epoch} help={s.not_ready ? "No task is worked out while the topology cannot run." : s.assignment_epoch === s.epoch ? "The same epoch: the tasks were worked out for the group as it is." : "Older than the group: the tasks are worked out at the next heartbeat."} />
        <Big label="Topology epoch" value={s.topology_epoch ?? "—"} help="The version of the topology the application sent; a member with another one is fenced." />
        <Big label="Tasks" value={tasks === null ? "—" : number(tasks)} help={<>Task p of a subtopology reads partition p of its topics. Assignor: {h.assignor ?? "sticky"}.</>} />
      </div>
      <section>
        <h2 className="text-[17px] font-semibold text-bright">Topology</h2>
        <p className="mt-1 mb-3 max-w-[52rem] text-[13.5px] text-muted-foreground">
          The application's processing, cut where records are re-keyed. Each subtopology reads its topics and hands records to the next through a repartition topic; a state store is backed by a
          changelog topic, read back when its task moves to another member.
        </p>
        {s.subtopologies.length === 0 ? (
          <p className="rounded-xl border border-dashed bg-card/60 p-8 text-center text-[13.5px] text-muted-foreground">No member has sent a topology yet.</p>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {s.subtopologies.map((t) => (
              <Subtopology key={t.id} s={t} />
            ))}
          </div>
        )}
      </section>
      {s.internal.length > 0 && (
        <section>
          <h2 className="text-[17px] font-semibold text-bright">Internal topics</h2>
          <p className="mt-1 mb-3 max-w-[52rem] text-[13.5px] text-muted-foreground">
            Made by the broker for the group when the topology is first ready, with as many partitions as the subtopology has tasks. They are ordinary topics: deleting the group leaves them.
          </p>
          <DataTable data={s.internal} columns={internalColumns(wide)} rowId={(t) => t.name} aria-label="Internal topics" maxHeight={320} />
        </section>
      )}
      <section>
        <h2 className="text-[17px] font-semibold text-bright">Members and their tasks</h2>
        <p className="mt-1 mb-3 max-w-[52rem] text-[13.5px] text-muted-foreground">
          Each member is one stream thread; members of one process share its process id. An active task processes records; a standby keeps a copy of its state from the changelog; a warm-up
          catches up before it takes over. A task changes member only after the old one gives it up.
        </p>
        <DataTable data={s.members} columns={memberColumns(h.members, onOpen, wide)} rowId={(m) => m.id} aria-label="Members" empty={s.live ? "Nobody is in this group." : "Nobody is in this group: the broker has its topology from before it was started again."} maxHeight={420} rowHeight={56} />
      </section>
      <div className="grid gap-6 min-[1700px]:grid-cols-2">
        <Panel title="Where a streams group can be">
          <Rules rows={WHERE.map(([state, w, sub]) => ({ ok: s.state === state, here: s.state === state, title: w, sub: s.state === state ? `${sub} This group, now.` : sub, value: s.state === state ? "now" : "" }))} />
        </Panel>
        <Panel title="Reads">
          <ReadsTable topics={v.topics} />
          <p className="mt-3 text-[12.5px] text-muted-foreground">
            Offsets are committed on the topics the tasks read (sources and repartition topics), with the member epoch. Reset them with{" "}
            <code className="font-mono">kafka-streams-groups --reset-offsets --group {h.id}</code> or Reset offsets above, while the group is empty.
          </p>
        </Panel>
      </div>
    </div>
  );
}
