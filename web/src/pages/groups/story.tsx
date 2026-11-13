// The Story tab (boards group-story, group-story-calm, story-rebalance, story-flapping): why the group rebalanced, one
// generation at a time from the broker's own ring of notes. The Now card counts the rejoin deadline down here, in its own
// slot; a rebalance opens as a Sheet (`?generation=`), and the flapping rule held against the group as another (`?rule=1`).
import { getRouteApi, Link } from "@tanstack/react-router";
import { SearchIcon } from "lucide-react";
import { useStory } from "@/api/groups";
import type { GroupHead } from "@/api/types/GroupHead";
import type { StoryGen } from "@/api/types/StoryGen";
import type { StoryNow } from "@/api/types/StoryNow";
import type { StoryTone } from "@/api/types/StoryTone";
import type { StoryView } from "@/api/types/StoryView";
import type { StoryWho } from "@/api/types/StoryWho";
import { type Stat, StatStrip } from "@/components/app/stat";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import { Countdown } from "./members";
import { useRoutedOverlay } from "./overlay";
import { Callout, Facts, Panel, PhaseWord, Rules } from "./parts";
import { ago, CAUSE_HELP, CAUSE_PHRASE, CAUSE_TITLE, CAUSES, causeListed, clockMs, clockTenths, clockUtc, holdsPhrase, lasted, memberLabel, partsPhrase, seconds, spanPrecise, stamp } from "./words";

const route = getRouteApi("/groups/$group/story");

function Who({ w }: { w: StoryWho | null }) {
  if (!w) return <>a member</>;
  return (
    <b className="font-semibold text-bright" title={w.member}>
      {memberLabel(w.member)}
    </b>
  );
}

function genTitle(g: StoryGen): string {
  const word = g.protocol === "classic" ? "Generation" : "Epoch";
  return g.forming ? `${word} ${g.generation} is forming` : `${word} ${g.generation}`;
}

function What({ g }: { g: StoryGen }) {
  const w = g.who;
  const consumer = g.protocol === "consumer";
  const members = ` Members ${g.before} to ${g.after}.`;
  let body: React.ReactNode;
  switch (g.cause) {
    case "member_joined":
      body = (
        <>
          <Who w={w} />
          {g.before === 0 ? " joined an empty group and led it." : " joined."}
          {members}
        </>
      );
      break;
    case "member_left":
      body = (
        <>
          <Who w={w} />
          {consumer ? " left the group." : " sent LeaveGroup."}
          {members}
          {w?.reason && (
            <>
              {" "}It said: <q>{w.reason}</q>.
            </>
          )}
        </>
      );
      break;
    case "session_expired":
      body = (
        <>
          <Who w={w} />
          {w?.session_ms ? ` sent no heartbeat for ${seconds(w.session_ms)}, its session timeout.` : " sent no heartbeat within its session timeout."}
          {members}
        </>
      );
      break;
    case "topic_changed":
      body = "A topic this group reads changed: partitions were added, or a topic was made or deleted. The broker moved the group on and worked out who holds what again.";
      break;
    case "protocols_changed":
      body = (
        <>
          <Who w={w} /> joined again offering other protocols than before.{members}
        </>
      );
      break;
    case "leader_rejoined":
      body = (
        <>
          The leader, <Who w={w} />, joined again with what it had. Kafka rebalances when the leader does; clients do it when a topic they read changes.
        </>
      );
      break;
    case "rebalance_timeout":
      body = (
        <>
          <Who w={w} /> did not give up what it was told to within its rebalance timeout, and was removed.{members}
        </>
      );
      break;
    case "subscription_changed":
      body = (
        <>
          <Who w={w} /> asked for other topics, a pattern or an assignor.{members}
        </>
      );
      break;
    case "rights_changed":
      body = (
        <>
          A right to read a topic was given to, or taken from, the principal of <Who w={w} />.
        </>
      );
      break;
    case "became_classic":
      body = "The last consumer-protocol member left, so the group is a classic group again and every member joins again.";
      break;
    case "became_consumer":
      body = "A member of the new protocol joined a classic group of consumers: it is a consumer group now, its members staying.";
      break;
    case "forced":
      body = "A rebalance was forced from the Lab. No member joined or left and no topic changed; the group moved on as it does when one does.";
      break;
  }
  const settled = g.settled_ms ?? g.formed_ms;
  return (
    <>
      {body}
      {settled !== null && ` Settled in ${spanPrecise(settled)}.`}
    </>
  );
}

function Then({ g, newest }: { g: StoryGen; newest: boolean }) {
  const present = newest && !g.forming;
  const alone = g.holds.length === 1;
  const held = g.holds.map((h) => holdsPhrase(h.member, h.topics, present, alone));
  return (
    <>
      {g.dropped.map((d) => (
        <span key={d.who.member}>
          <Who w={d.who} /> did not join again in {spanPrecise(d.waited_ms)} and was dropped.{" "}
        </span>
      ))}
      {held.length > 0 && `${held.join("; ")}. `}
      {g.lasted_ms !== null && (newest ? `Lasted ${lasted(g.lasted_ms)} so far.` : `Lasted ${lasted(g.lasted_ms)}.`)}
    </>
  );
}

const DOT: Record<StoryTone, string> = { warn: "border-warn", bad: "border-bad", idle: "border-muted-foreground", plain: "border-primary" };

function Generation({ g, head, newest, onOpen, fetchedAt }: { g: StoryGen; head: GroupHead; newest: boolean; onOpen?: (n: number, from: HTMLElement) => void; fetchedAt: number }) {
  const members = g.forming && g.members.length === 0 ? head.members.map((m) => m.id) : g.members;
  const total = g.forming && g.members.length === 0 ? members.length : g.members_total;
  const gone = g.forming && g.cause === "member_left" ? g.who?.member : undefined;
  const r = head.rebalance;
  return (
    <li className="group/gen relative grid grid-cols-[5.5rem_1.25rem_minmax(0,1fr)] gap-x-3 pb-7 last:pb-0" data-testid="generation" data-generation={g.generation}>
      <time className="pt-0.5 font-mono text-[12.5px] text-muted-foreground" dateTime={new Date(g.at_ms).toISOString()}>
        {clockUtc(g.at_ms)}
      </time>
      <span className="relative flex justify-center">
        <span className={cn("relative z-10 mt-1 size-3 rounded-full border-2 bg-background", DOT[g.tone])} aria-hidden />
        <span className="absolute top-5 bottom-[-0.25rem] w-px bg-border group-last/gen:hidden" aria-hidden />
      </span>
      <div className="min-w-0 text-[13.5px]">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="secondary" className="rounded-md bg-accent font-normal text-primary">
            {CAUSE_TITLE[g.cause]}
          </Badge>
          <b className="font-semibold text-bright">{genTitle(g)}</b>
        </div>
        <p className="mt-1.5 text-muted-foreground">
          <What g={g} />
        </p>
        {g.forming && newest ? (
          r &&
          r.waiting_for.length > 0 && (
            <p className="mt-1.5 text-muted-foreground">
              Not rejoined:{" "}
              {r.waiting_for.map((m, i) => (
                <span key={m}>
                  {i > 0 && ", "}
                  <b className="font-semibold text-bright" title={m}>
                    {memberLabel(m)}
                  </b>
                </span>
              ))}
              . <Countdown deadline={fetchedAt + r.remaining_ms} className="min-w-0 text-left" /> left of {spanPrecise(r.timeout_ms)}.
            </p>
          )
        ) : (
          <p className="mt-1.5 text-muted-foreground">
            <Then g={g} newest={newest} />
          </p>
        )}
        {(members.length > 0 || gone) && (
          <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
            <span className="text-[12.5px] text-muted-foreground">In the group:</span>
            {members.map((m) => (
              <Badge key={m} variant="outline" className="rounded-full font-mono text-[11.5px] font-normal" title={m}>
                {memberLabel(m)}
              </Badge>
            ))}
            {total > members.length && (
              <Badge variant="outline" className="rounded-full text-[11.5px] font-normal">
                and {total - members.length} more
              </Badge>
            )}
            {gone && (
              <Badge variant="outline" className="rounded-full font-mono text-[11.5px] font-normal text-muted-foreground" title={gone}>
                {memberLabel(gone)} left
              </Badge>
            )}
          </div>
        )}
        {onOpen && (
          <button type="button" onClick={(e) => onOpen(g.generation, e.currentTarget)} className="mt-2.5 rounded-sm text-[13px] font-medium text-primary hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="open-rebalance">
            Open this rebalance
          </button>
        )}
      </div>
    </li>
  );
}

function verdict(v: StoryView): string {
  const f = v.flap;
  if (f.flapping) return `Yes, by the rule: ${f.cycles} expired-then-joined cycles in the last 10 minutes. A member keeps timing out and coming back.`;
  const expired = f.expiries === 0 ? "no session has expired" : f.expiries === 1 ? "one session expiry" : `${f.expiries} session expiries`;
  const watch = f.rows.find((r) => !r.ok && r.title.startsWith("A member joined and left"));
  return `Not by the rule: ${expired} since the broker started, and a clean leave never counts.${watch ? ` A member did join and leave within ${watch.value}; worth a look if it repeats.` : ""}`;
}

/** What the group is doing now. The time left is a countdown here, never a number in the sentence. */
function Now({ v, fetchedAt }: { v: StoryView; fetchedAt: number }) {
  const n: StoryNow = v.now;
  const word = n.protocol === "classic" ? "Generation" : "Epoch";
  let sentence: React.ReactNode;
  if (n.phase === "rebalancing" && n.began_ms !== null) {
    const how = n.cause === "member_left" ? " left" : n.cause === "session_expired" ? " timed out" : n.cause === "member_joined" ? " joined" : null;
    sentence = (
      <>
        {word} {n.generation} began at <b className="font-mono text-bright">{clockTenths(n.began_ms)}</b>
        {n.who && how ? (
          <>
            {" "}when <Who w={n.who} />
            {how}
          </>
        ) : n.cause ? (
          ` (${CAUSE_PHRASE[n.cause]})`
        ) : null}
        .{" "}
        {n.waiting_for.length === 0 ? (
          "Every member has joined again; the leader is working out who holds what."
        ) : (
          <>
            It is waiting for{" "}
            {n.waiting_for.map((m, i) => (
              <span key={m}>
                {i > 0 && ", "}
                <b className="font-mono text-bright">{m}</b>
              </span>
            ))}
            .
          </>
        )}
      </>
    );
  } else if (n.phase === "empty") {
    const g = v.generations[0];
    sentence = `Nobody is in the group.${g ? ` The last rebalance, ${word.toLowerCase()} ${g.generation}, ended with it empty.` : ""}`;
  } else if (n.began_ms !== null) {
    sentence = (
      <>
        {word} {n.generation} began at <b className="font-mono text-bright">{clockTenths(n.began_ms)}</b>
        {n.who && (
          <>
            {" "}when <Who w={n.who} />
            {n.cause === "member_left" ? " left" : n.cause === "session_expired" ? " timed out" : " joined"}
          </>
        )}
        . It has run for {n.held_ms !== null ? lasted(n.held_ms) : ""}.
      </>
    );
  } else sentence = "The broker has not seen this group rebalance since it started.";
  const deadline = n.remaining_ms !== null ? fetchedAt + n.remaining_ms : null;
  const pct = n.remaining_ms !== null && n.timeout_ms ? Math.min(100, Math.floor((Math.min(n.remaining_ms, n.timeout_ms) * 100) / n.timeout_ms)) : 0;
  return (
    <Panel title="Now" aside={<PhaseWord phase={n.phase} />} className="mb-8" data-testid="story-now">
      <p className="text-[14px] text-muted-foreground">{sentence}</p>
      {/* The rejoin deadline's slot is always there, so the card is the same height whatever the group is doing. */}
      <div className="mt-4 flex min-h-6 items-center gap-4">
        {n.phase === "rebalancing" && deadline !== null && n.timeout_ms !== null ? (
          <>
            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-track" aria-hidden>
              <div className="h-full rounded-full bg-warn" style={{ width: `${pct}%` }} />
            </div>
            <span className="text-[13px] whitespace-nowrap text-muted-foreground">
              Rejoin deadline <Countdown deadline={deadline} className="text-bright" /> of {spanPrecise(n.timeout_ms)}
            </span>
          </>
        ) : (
          <span className="text-[13px] text-muted-foreground">No rejoin deadline: nothing is waiting.</span>
        )}
      </div>
    </Panel>
  );
}

function Causes({ v }: { v: StoryView }) {
  const classic = v.head.protocol === "classic";
  const seen = (c: string) => v.causes.find(([k]) => k === c)?.[1] ?? 0;
  return (
    <Panel title="Where the cause comes from" aside={<Badge className="rounded-md text-[10.5px] tracking-wide uppercase">New</Badge>}>
      <p className="text-[13px] text-muted-foreground">
        Kafka's protocol has no call that says why a group rebalanced; a client sees only that it must rejoin. The broker knows, because it is the one deciding. It records the cause at that moment, with the member and how long the rebalance took.
        {classic && " A topic that changed reaches a classic group as its leader joining again, which is how the cause is told here."}
      </p>
      <div className="mt-4 border-t pt-4 text-[11.5px] font-medium tracking-[0.06em] text-muted-foreground uppercase">The causes it can name</div>
      <ul className="mt-3 flex flex-col gap-2.5">
        {CAUSES.filter((c) => causeListed(classic, c) || seen(c) > 0).map((c) => (
          <li key={c} className="text-[13px] text-muted-foreground">
            <Badge variant="secondary" className="mr-2 rounded-md bg-accent font-normal text-primary">
              {CAUSE_TITLE[c]}
            </Badge>
            {CAUSE_HELP[c]}
            {seen(c) > 0 && <b className="ml-1 font-mono text-bright">· {seen(c)} here</b>}
          </li>
        ))}
      </ul>
    </Panel>
  );
}

function Calm({ v, fetchedAt }: { v: StoryView; fetchedAt: number }) {
  const c = v.calm;
  if (!c) {
    return (
      <Panel title="What a calm group looks like">
        <p className="text-[13px] text-muted-foreground">A quiet story is a good one: one rebalance when the first member joined, and nothing since. If a group's lag grows and its story is quiet, the cause is not rebalancing: look at its Lag, and at how many partitions one process holds.</p>
      </Panel>
    );
  }
  return (
    <Panel title="What a calm group looks like" aside={<PhaseWord phase="stable" />} data-testid="story-calm">
      <p className="text-[13px] text-muted-foreground">
        <Link to="/groups/$group/story" params={{ group: c.group }} className="font-mono font-semibold text-primary hover:underline">
          {c.group}
        </Link>
        , for contrast: {c.count} {c.count === 1 ? "rebalance" : "rebalances"} since the broker started, and nothing has moved since {clockUtc(c.generation.at_ms)}. Its Story tab holds {c.count === 1 ? "only this event." : "these events."}
      </p>
      <ol className="group/story mt-4">
        <Generation g={c.generation} head={v.head} newest fetchedAt={fetchedAt} />
      </ol>
      <p className="mt-4 border-t pt-3 text-[13px] text-muted-foreground">
        A quiet story is a good one. No member has come, gone or timed out since {clockUtc(c.generation.at_ms)}, so the lag on that group is not caused by rebalancing. Look at{" "}
        <Link to="/groups/$group/lag" params={{ group: c.group }} className="text-primary hover:underline">
          its Lag
        </Link>{" "}
        and at how many partitions one process holds.
      </p>
    </Panel>
  );
}

function api(g: StoryGen): string {
  if (g.protocol === "consumer") return "ConsumerGroupHeartbeat";
  return g.cause === "member_left" ? "LeaveGroup" : g.cause === "session_expired" ? "Heartbeat" : "JoinGroup";
}

function RebalanceSheet({ v, generation, onClose, fetchedAt, returnFocus }: { v: StoryView; generation?: number; onClose: () => void; fetchedAt: number; returnFocus: (e: Event) => void }) {
  const g = generation !== undefined ? v.generations.find((x) => x.generation === generation) : undefined;
  const newest = !!g && v.generations[0] === g;
  const word = v.head.protocol === "classic" ? "Generation" : "Epoch";
  const rows: [string, React.ReactNode][] = [];
  if (g) {
    rows.push([
      "Who",
      g.who ? (
        <span className="flex flex-col">
          <span className="font-mono" title={g.who.member}>
            {memberLabel(g.who.member)}
            {g.more > 0 && ` and ${g.more} more`}
          </span>
          <span className="text-[12px] text-muted-foreground">
            {g.who.reason ? (
              <>
                {api(g)}, said: <q>{g.who.reason}</q>
              </>
            ) : g.cause === "session_expired" ? (
              `sent no heartbeat${g.who.session_ms ? ` for ${seconds(g.who.session_ms)}` : ""}`
            ) : (
              `${api(g)}, sent no reason`
            )}
          </span>
        </span>
      ) : (
        <span className="text-muted-foreground">No member: {CAUSE_TITLE[g.cause]}</span>
      ),
    ]);
    rows.push(["Began", <span className="font-mono">{clockMs(g.at_ms)}</span>]);
    if (g.wait_ms !== null)
      rows.push([
        "Deadline",
        <span className="flex flex-col">
          <span className="font-mono">{clockMs(g.at_ms + g.wait_ms)}</span>
          <span className="text-[12px] text-muted-foreground">the longest rebalance timeout among members, {spanPrecise(g.wait_ms)}</span>
        </span>,
      ]);
    if (g.forming && newest && v.now.waiting_for.length > 0)
      rows.push([
        "Not rejoined",
        <span className="flex flex-col font-mono">
          {v.now.waiting_for.map((m) => {
            const mem = v.head.members.find((x) => x.id === m);
            return (
              <span key={m} title={m}>
                {memberLabel(m)} <span className="font-sans text-[12px] text-muted-foreground">last heard {mem?.heard_ago_ms !== null && mem?.heard_ago_ms !== undefined ? ago(mem.heard_ago_ms) : "not told"}</span>
              </span>
            );
          })}
        </span>,
      ]);
    for (const d of g.dropped) rows.push(["Dropped", <span className="font-mono">{memberLabel(d.who.member)} did not join in {spanPrecise(d.waited_ms)}</span>]);
    rows.push(["Members", `${g.before} before, ${g.after} after`]);
    rows.push([
      "What moves",
      g.moves.length === 0 ? (
        <span className="text-muted-foreground">not kept: the generation before is older than the ring, or this one moved nothing</span>
      ) : (
        <span className="flex flex-col gap-1">
          {g.moves.map(([member, topics]) => (
            <span key={member}>
              <span className="font-mono">{partsPhrase(topics)}</span>
              <span className="block text-[12px] text-muted-foreground">held by {memberLabel(member)}, go to whoever is in the group when it forms</span>
            </span>
          ))}
        </span>
      ),
    ]);
    if (g.forming && newest) rows.push(["If nobody rejoins", "The group goes Empty. Offsets stay."]);
    else if (g.lasted_ms !== null) rows.push(["Lasted", <span className="font-mono">{lasted(g.lasted_ms)}{newest ? " so far" : ""}</span>]);
  }
  const status = g ? (g.forming && newest ? (v.now.remaining_ms !== null && v.now.timeout_ms !== null ? <span className="text-[13px] font-semibold text-warn">Waiting, <Countdown deadline={fetchedAt + v.now.remaining_ms} className="min-w-0" /> of {spanPrecise(v.now.timeout_ms)} left</span> : <span className="text-[13px] font-semibold text-warn">Forming</span>) : <span className="text-[13px] font-semibold text-good">{(g.settled_ms ?? g.formed_ms) !== null ? `Settled in ${spanPrecise((g.settled_ms ?? g.formed_ms)!)}` : "Began"}</span>) : null;
  return (
    <Sheet open={generation !== undefined} onOpenChange={(o) => !o && onClose()}>
      <SheetContent onCloseAutoFocus={returnFocus} className="w-full sm:max-w-xl" data-testid="rebalance-sheet">
        <SheetHeader>
          <SheetTitle>{g ? `${word} ${g.generation}` : `${word} ${generation}`}</SheetTitle>
          <SheetDescription asChild>
            <div className="flex flex-wrap items-center gap-2">
              {g ? (
                <>
                  <Badge variant="secondary" className="rounded-md bg-accent font-normal text-primary">
                    {CAUSE_TITLE[g.cause]}
                  </Badge>
                  {status}
                </>
              ) : (
                "The ring does not hold this rebalance: it was let go, or there never was one."
              )}
            </div>
          </SheetDescription>
        </SheetHeader>
        {g && (
          <div className="flex flex-col gap-6 px-4 pb-6">
            <Facts rows={rows} />
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" asChild>
                <Link to="/groups/$group/wire" params={{ group: v.head.id }} search={{ kind: g.cause === "member_left" || g.cause === "member_joined" ? "join" : undefined }}>
                  See the {api(g)} on the wire
                </Link>
              </Button>
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

function RuleSheet({ v, open, onClose, returnFocus }: { v: StoryView; open: boolean; onClose: () => void; returnFocus: (e: Event) => void }) {
  const f = v.flap;
  const ev = (tone: StoryTone, at: string, cause: string, head: string, line: string) => (
    <li className="grid grid-cols-[3.5rem_1.25rem_minmax(0,1fr)] gap-x-3 pb-5 last:pb-0">
      <span className="font-mono text-[12.5px] text-muted-foreground">{at}</span>
      <span className="flex justify-center">
        <span className={cn("mt-1 size-3 rounded-full border-2", DOT[tone])} aria-hidden />
      </span>
      <div className="text-[13px]">
        <Badge variant="secondary" className="mr-2 rounded-md bg-accent font-normal text-primary">
          {cause}
        </Badge>
        <b className="text-bright">{head}</b>
        <p className="mt-1 text-muted-foreground">{line}</p>
      </div>
    </li>
  );
  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()}>
      <SheetContent onCloseAutoFocus={returnFocus} className="w-full sm:max-w-xl" data-testid="rule-sheet">
        <SheetHeader>
          <SheetTitle>The rule, against {v.head.id}</SheetTitle>
          <SheetDescription>Three or more expired-then-joined cycles in 10 minutes.</SheetDescription>
        </SheetHeader>
        <div className="flex flex-col gap-6 overflow-y-auto px-4 pb-6">
          <p className="text-[13.5px] text-muted-foreground">
            A group is flapping when the same member keeps timing out and coming back. <span className="font-mono">{v.head.id}</span> is <b className="text-bright">{f.flapping ? "flapping" : "not"}</b>
            {f.flapping ? `: ${f.cycles} expired-then-joined cycles in the last 10 minutes.` : ` flapping: ${f.expiries} session ${f.expiries === 1 ? "expiry" : "expiries"} since the broker started, and a member that leaves cleanly never counts.`}
          </p>
          <Rules rows={f.rows.map((r) => ({ ok: r.ok, blocking: !r.ok, title: r.title, sub: r.detail, value: r.value }))} />
          <div className="rounded-xl border bg-card p-4">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
              <h3 className="text-[14.5px] font-semibold text-bright">What it looks like when it is</h3>
              <Badge variant="outline" className="rounded-full font-normal">
                Illustration, not your group
              </Badge>
            </div>
            <ol>
              {ev("bad", "+0:00", "Session expired", "Member timed out", "No heartbeat for 10.0 s, its session timeout.")}
              {ev("plain", "+0:04", "Member joined", "Same member is back", "The group rebalances again.")}
              {ev("bad", "+0:30", "Session expired", "Member timed out", "30.1 s after the last expiry.")}
              {ev("plain", "+0:34", "Member joined", "Same member is back", "The pattern is named at the third cycle.")}
            </ol>
          </div>
          <Callout>
            <b className="font-semibold text-bright">Where to look.</b> The app spends longer between polls than its session allows: slow processing, a pause, a debugger. Raise <code className="font-mono">session.timeout.ms</code> or do less between polls.
          </Callout>
        </div>
      </SheetContent>
    </Sheet>
  );
}

function quiet(v: StoryView): boolean {
  return v.in_memory && v.head.phase === "stable" && v.generations.length === 1 && !v.generations[0]!.forming;
}

export function GroupStoryPage() {
  const { group: id } = route.useParams();
  const search = route.useSearch();
  const res = useStory(id);
  const gen = useRoutedOverlay("generation");
  const rule = useRoutedOverlay("rule");
  const v = res.data;
  if (!v) return <Skeleton className="h-96 rounded-xl" />;
  const forming = v.generations[0]?.forming ?? false;
  const stats: Stat[] = [
    { label: "Rebalances kept", value: String(v.generations.length), ch: 5 },
    { label: "Newest", value: v.generations.length === 0 ? "none" : forming ? "Forming" : "Settled", ch: 9 },
    { label: "Causes", value: String(v.causes.length), ch: 3 },
    v.since_ms !== null ? { label: "Oldest note", value: stamp(v.since_ms, v.head.now_ms), unit: "UTC", mono: true, ch: 11 } : { label: "Oldest note", value: "none", mono: true, ch: 11 },
  ];
  const oldest = v.generations[v.generations.length - 1]?.generation ?? 0;
  const calm = quiet(v);
  return (
    <div data-testid="group-story">
      <StatStrip stats={stats} className="mb-8" />
      <div className="grid gap-8 xl:grid-cols-[minmax(0,1fr)_25rem]">
        <div className="min-w-0">
          {!calm && <Now v={v} fetchedAt={res.dataUpdatedAt} />}
          <h2 className="mb-5 text-[17px] font-semibold text-bright">Generations, newest first</h2>
          {v.generations.length === 0 ? (
            <p className="text-[13.5px] text-muted-foreground">Nothing to tell yet. The first member to join, leave or time out is the first line of the story.</p>
          ) : (
            <ol className="group/story" data-testid="story-timeline">
              {v.generations.map((g, i) => (
                <Generation key={`${g.generation}-${g.at_ms}`} g={g} head={v.head} newest={i === 0} onOpen={(n, from) => gen.open(n, from)} fetchedAt={res.dataUpdatedAt} />
              ))}
            </ol>
          )}
          <p className="mt-6 text-[13px] text-muted-foreground">
            {v.full && oldest > 1 ? `Generations 1 to ${oldest - 1} are older than the ring keeps: ` : "This is everything since the broker started: the ring keeps "}
            {v.events_per_group} events per group, emptied at a restart.
          </p>
        </div>
        <div className="flex min-w-0 flex-col gap-6">
          {calm ? (
            <Panel title="A quiet story is a good one" data-testid="story-quiet">
              <p className="text-[13px] text-muted-foreground">
                No member has come, gone or timed out since {clockUtc(v.generations[0]!.at_ms)}.{" "}
                {v.head.lag_total > 0 ? (
                  <>
                    So the lag on this group is not caused by rebalancing: look at{" "}
                    <Link to="/groups/$group/lag" params={{ group: id }} className="text-primary hover:underline">
                      its Lag
                    </Link>{" "}
                    and at how many partitions one process holds.
                  </>
                ) : (
                  "So there is nothing in the story to explain."
                )}
              </p>
            </Panel>
          ) : (
            <>
              <Causes v={v} />
              <Panel title="Is it flapping?" data-testid="story-flapping">
                <p className="text-[13px] text-muted-foreground" data-testid="flap-verdict">
                  {verdict(v)}
                </p>
                <Button variant="outline" className="mt-4 bg-card" onClick={(e) => rule.open(1, e.currentTarget)} data-testid="check-rule">
                  <SearchIcon /> Check the rule
                </Button>
              </Panel>
              <Calm v={v} fetchedAt={res.dataUpdatedAt} />
            </>
          )}
        </div>
      </div>
      <RebalanceSheet v={v} generation={search.generation} onClose={gen.close} fetchedAt={res.dataUpdatedAt} returnFocus={gen.returnFocus} />
      <RuleSheet v={v} open={search.rule === 1} onClose={rule.close} returnFocus={rule.returnFocus} />
    </div>
  );
}
