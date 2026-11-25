// The rehearsals the Lab will offer, and what each stands for in production. Until doze-aws has the fault engine and the forced events
// (docs/port/aws/lab.md, parts 2 and 3), the cards are drawn off, with the reason. When it has, `LAB_ENGINE` is read from the server and the
// words come from its guide (`/api/v1/lab/guide`); this list is the shape they are drawn in.
import { BanIcon, CloudOffIcon, HourglassIcon, type LucideIcon, MoonIcon, PlugZapIcon, ServerCrashIcon, SnowflakeIcon, TimerResetIcon, TimerIcon, GaugeIcon, CalendarClockIcon } from "lucide-react";

/** Whether this doze-aws has the fault engine and the forced events. */
export const LAB_ENGINE = false;
export const NEEDS_NEWER = "Needs a newer doze-aws";

export type Rehearsal = { id: string; title: string; icon: LucideIcon; lede: string; production: string };

/** Rules: calls the app sends to the endpoint are slowed, answered with an error, or dropped, for a time. */
export const RULES: readonly Rehearsal[] = [
  { id: "slow", title: "Make calls slow", icon: TimerIcon, lede: "Hold the answer of the calls that match, then let them through, to see how your client's timeouts behave.", production: "A busy region, a cold start or a long network path: the answer comes, late." },
  { id: "throttle", title: "Throttle calls", icon: GaugeIcon, lede: "Answer the calls that match with the service's throttling error.", production: "A quota or a rate limit. SDKs retry throttling errors with backoff and give up after their attempts; a client that retries at once makes it worse." },
  { id: "unavailable", title: "Make a service unavailable", icon: CloudOffIcon, lede: "Answer with a 503, the way a service in trouble does.", production: "A service or one of its dependencies is down for a moment. SDKs retry these too, so a short outage should not reach your users." },
  { id: "internal", title: "Fail inside the service", icon: ServerCrashIcon, lede: "Answer with a 500, the service's own fault.", production: "AWS fails on its side. A call that is safe to repeat can be retried; for the others your client decides." },
  { id: "denied", title: "Deny access", icon: BanIcon, lede: "Answer with the service's access denied error.", production: "A policy changed, or a role lost a permission. Retrying does not help; a client should say what was denied." },
  { id: "drop", title: "Drop the connection", icon: PlugZapIcon, lede: "Close the connection with no answer.", production: "A dropped connection or a stuck load balancer: the client waits until its own timeout." },
];

/** Forced events: something production causes, which here waits on a timer, made to happen now through the service's own code. */
export const EVENTS: readonly Rehearsal[] = [
  { id: "rest", title: "Rest a service now", icon: MoonIcon, lede: "Close a service the way idleness does, so the next call opens it again.", production: "Nothing like it: AWS does not rest a service. It shows what the first call after a quiet spell costs here." },
  { id: "schedule", title: "Fire a schedule now", icon: CalendarClockIcon, lede: "Run an EventBridge Scheduler schedule's target now, by the path its timer uses.", production: "Waiting for a schedule's time is slow; this runs its target at once so you can see what it does." },
  { id: "visibility", title: "Time out an in-flight message", icon: HourglassIcon, lede: "Let a message that a consumer holds be received again, as its visibility timeout would.", production: "A consumer crashed or ran too long: the message comes back, with a higher receive count." },
  { id: "ttl", title: "Run the expiry sweep", icon: TimerResetIcon, lede: "Run DynamoDB's expiry sweep for a table now.", production: "DynamoDB deletes expired items in the background some time after they expire, not at the moment they do." },
  { id: "cold", title: "Make the next call cold", icon: SnowflakeIcon, lede: "Drop a function's warm worker, so its next call is a cold start.", production: "The first call after a quiet spell or a new version starts a new execution environment, which takes longer." },
];

