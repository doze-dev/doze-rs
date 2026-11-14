// The words of the Groups pages, as the classic console writes them (crates/kafka-console/src/story/words.rs,
// groups_words.rs, format.rs and pages/groups/*.rs are the source of truth; the unit tests hold the same cases). Pure.
import type { GroupMemberRow } from "@/api/types/GroupMemberRow";
import type { GroupPart } from "@/api/types/GroupPart";
import type { GroupPhase } from "@/api/types/GroupPhase";
import type { StoryCause } from "@/api/types/StoryCause";
import type { Tone } from "@/components/app/status-dot";
import { clockUtc, number } from "../../lib/format";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const two = (n: number) => String(n).padStart(2, "0");

/** `Nov 6 00:14` (UTC), the classic `date_time` without its zone. */
export function dateTime(ms: number): string {
  const d = new Date(ms);
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()} ${two(d.getUTCHours())}:${two(d.getUTCMinutes())}`;
}

/** `22:14:21.771` (UTC). */
export function clockMs(ms: number): string {
  return `${clockUtc(ms)}.${String(((ms % 1000) + 1000) % 1000).padStart(3, "0")}`;
}

/** `22:14:21.7`: to a tenth of a second. */
export function clockTenths(ms: number): string {
  return `${clockUtc(ms)}.${Math.floor((((ms % 1000) + 1000) % 1000) / 100)}`;
}

/** The time of the day when it is today, else the date and time: the classic `stamp`. */
export function stamp(ms: number, now: number): string {
  const day = 86_400_000;
  return Math.floor(ms / day) === Math.floor(now / day) ? clockUtc(ms) : dateTime(ms);
}

/** `41 s`, `2 min`, `3 h 5 min`, `3 d 4 h`: the classic `span`. */
export function span(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const pair = (big: number, bu: string, rest: number, su: string) => (rest === 0 ? `${big} ${bu}` : `${big} ${bu} ${rest} ${su}`);
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86_400) return pair(Math.floor(s / 3600), "h", Math.floor((s % 3600) / 60), "min");
  return pair(Math.floor(s / 86_400), "d", Math.floor((s % 86_400) / 3600), "h");
}

/** `2.1 s`, `45 s`, then as `span`: tenths of a second below a minute. */
export function spanPrecise(ms: number): string {
  if (ms < 60_000) {
    if (ms % 1000 < 100) return `${Math.floor(ms / 1000)} s`;
    return `${Math.floor(ms / 1000)}.${Math.floor((ms % 1000) / 100)} s`;
  }
  return span(ms);
}

/** A timeout the way a person says it: `45 s`, `60 s`, `3 min`. */
export function seconds(ms: number): string {
  if (ms < 120_000 && ms % 1000 === 0) return `${ms / 1000} s`;
  if (ms < 60_000) return spanPrecise(ms);
  return span(ms);
}

/** `3 s ago`, `5 m ago`: the classic `ago`. */
export function ago(ms: number): string {
  const d = Math.max(0, ms);
  if (d < 1000) return `${Math.floor(d / 1000)}.${Math.floor((d % 1000) / 100)} s ago`;
  if (d < 60_000) return `${Math.floor(d / 1000)} s ago`;
  if (d < 3_600_000) return `${Math.floor(d / 60_000)} m ago`;
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)} h ago`;
  return `${Math.floor(d / 86_400_000)} d ago`;
}

/** To the nearest second under a minute (`3 s ago` for 2.9 s). */
export function agoNear(ms: number): string {
  return ms >= 1000 && ms < 60_000 ? `${Math.floor((ms + 500) / 1000)} s ago` : ago(ms);
}

/** How long a generation ran: `1.1 s`, `5 m 17 s`, `2 h 24 m`, `3 d 4 h`. */
export function lasted(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (ms < 60_000) return spanPrecise(ms);
  const m = Math.floor(s / 60);
  const h = Math.floor(s / 3600);
  const d = Math.floor(s / 86_400);
  if (s < 3600) return `${m} m ${s % 60} s`;
  if (s < 86_400) return `${h} h ${m % 60} m`;
  return `${d} d ${h % 24} h`;
}

/** `a, b and c`. */
export function listAnd(items: string[]): string {
  if (items.length <= 1) return items[0] ?? "";
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** `one` to `ten`, else the digits. */
export function numberWord(n: number): string {
  return ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"][n] ?? number(n);
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${number(n)} ${n === 1 ? one : many}`;
}

const UUID = /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;

/** A member id without the UUID the broker appends (`fulfilment-py-1-7b0f6a0e-...` is `fulfilment-py-1`). */
export function memberLabel(id: string): string {
  const cut = id.length - 37;
  if (cut <= 0) return id;
  const tail = id.slice(cut);
  return tail.startsWith("-") && UUID.test(tail.slice(1)) ? id.slice(0, cut) : id;
}

/** `clickstream-0 to 5`, `orders-0, orders-2`: partitions of topics, runs folded (the members tables). */
export function partList(ps: GroupPart[]): string {
  const runs: [string, number, number][] = [];
  for (const p of ps) {
    const last = runs[runs.length - 1];
    if (last && last[0] === p.topic && p.partition === last[2] + 1) last[2] = p.partition;
    else runs.push([p.topic, p.partition, p.partition]);
  }
  return runs
    .map(([t, a, b]) => (b - a === 0 ? `${t}-${a}` : b - a === 1 ? `${t}-${a}, ${t}-${b}` : `${t}-${a} to ${b}`))
    .join(", ");
}

/** `payments.settled-0, -1 and pick-lists-0` (the Story's phrase). */
export function partsPhrase(topics: [string, number[]][]): string {
  const items: string[] = [];
  for (const [topic, partitions] of topics) {
    const ps = [...new Set(partitions)].sort((a, b) => a - b);
    const runs: [number, number][] = [];
    for (const p of ps) {
      const last = runs[runs.length - 1];
      if (last && last[1] + 1 === p) last[1] = p;
      else runs.push([p, p]);
    }
    let first = true;
    for (const [lo, hi] of runs) {
      if (hi - lo >= 2) {
        items.push(`${first ? `${topic}-` : "-"}${lo} to ${hi}`);
        first = false;
      } else {
        for (let p = lo; p <= hi; p++) {
          items.push(first ? `${topic}-${p}` : `-${p}`);
          first = false;
        }
      }
    }
  }
  return listAnd(items);
}

/** `py-1 held payments.settled-0, -1 and pick-lists-0`. */
export function holdsPhrase(member: string, topics: [string, number[]][], present: boolean, alone: boolean): string {
  const who = alone && present ? "It" : memberLabel(member);
  const verb = present ? "holds" : "held";
  return topics.length === 0 ? `${who} ${verb} nothing` : `${who} ${verb} ${partsPhrase(topics)}`;
}

export const CAUSES: StoryCause[] = [
  "member_joined",
  "member_left",
  "session_expired",
  "topic_changed",
  "protocols_changed",
  "leader_rejoined",
  "rebalance_timeout",
  "subscription_changed",
  "rights_changed",
  "became_classic",
  "became_consumer",
  "forced",
];

export const CAUSE_TITLE: Record<StoryCause, string> = {
  member_joined: "Member joined",
  member_left: "Member left",
  session_expired: "Session expired",
  topic_changed: "Topic changed",
  protocols_changed: "Rejoined with new protocols",
  leader_rejoined: "Leader rejoined",
  rebalance_timeout: "Rebalance timeout",
  subscription_changed: "Subscription changed",
  rights_changed: "Rights changed",
  became_classic: "Became classic",
  became_consumer: "Became a consumer group",
  forced: "Forced from the Lab",
};

export const CAUSE_PHRASE: Record<StoryCause, string> = {
  member_joined: "a member joined",
  member_left: "a member left",
  session_expired: "a member timed out",
  topic_changed: "a topic changed",
  protocols_changed: "a member rejoined with other protocols",
  leader_rejoined: "the leader rejoined",
  rebalance_timeout: "members did not rejoin in time",
  subscription_changed: "a member changed what it subscribes to",
  rights_changed: "a right to read a topic changed",
  became_classic: "the group became a classic group again",
  became_consumer: "the group became a consumer group",
  forced: "a rebalance was forced from the Lab",
};

export const CAUSE_HELP: Record<StoryCause, string> = {
  member_joined: "a new member, or the first of an empty group",
  member_left: "LeaveGroup, sent by the client",
  session_expired: "no heartbeat within the session timeout",
  topic_changed: "partitions added to a subscribed topic, or one made or deleted (consumer-protocol groups)",
  protocols_changed: "a member changed what it offers",
  leader_rejoined: "the leader of a running group joined again; clients do it when a topic they read changes",
  rebalance_timeout: "members did not rejoin in time, or give up what they were told to",
  subscription_changed: "a consumer-protocol member asked for other topics, a pattern or an assignor",
  rights_changed: "a right to describe a topic was given to a member's principal or taken from it",
  became_classic: "the last consumer-protocol member left a group that still had classic members",
  became_consumer: "a consumer-protocol member joined a classic group of consumers",
  forced: "someone forced a rebalance from the Lab: no member or topic caused it",
};

/** The causes a group of this protocol can have, listed whether or not they have happened. */
export function causeListed(classic: boolean, c: StoryCause): boolean {
  return classic
    ? ["member_joined", "member_left", "session_expired", "protocols_changed", "leader_rejoined", "rebalance_timeout"].includes(c)
    : ["member_joined", "member_left", "session_expired", "topic_changed", "subscription_changed", "rebalance_timeout"].includes(c);
}

export const PHASE_WORD: Record<GroupPhase, string> = { empty: "Empty", stable: "Stable", rebalancing: "Rebalancing", assigning: "Assigning", reconciling: "Reconciling" };

export function phaseTone(p: GroupPhase): Tone {
  return p === "stable" ? "ok" : p === "empty" ? "idle" : "warn";
}

/** When a member was last heard from: the time, or `not told`. */
export function heard(now: number, m: GroupMemberRow): string {
  return m.heard_ago_ms === null ? "not told" : stamp(now - m.heard_ago_ms, now);
}

export { clockUtc, number };
