// What the EventBridge pages say, as pure functions: counts and lists, how a pattern, a target and a retention read in a table, and
// the sentences over the lists.
import type { ArchiveItem, ApiDestItem, ReplayItem, RuleItem, TargetItem } from "./types";

/** `1 rule`, `3 rules`. */
export const count = (n: number, one: string, many = `${one}s`): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
/** `a`, `a and b`, `a, b and c`. */
export const list = (items: readonly string[]): string => (items.length <= 1 ? items.join("") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`);

/** `30 days`, `1 day`, `Indefinite` (0 or none is how EventBridge says the events are kept for ever). */
export const retentionWords = (days: number | null | undefined): string => (days ? count(days, "day") : "Indefinite");

/** The name of a resource in its ARN: after the last `/` or `:`. */
export const arnName = (arn: string): string => arn.split(/[/:]/).filter(Boolean).pop() ?? arn;

/** The name of a connection in its ARN (`connection/<name>/<id>`). */
export const connectionName = (arn: string): string => arn.split("connection/")[1]?.split("/")[0] ?? arnName(arn);

/** The bus an archive takes events from, by its ARN. */
export const busOfArn = (arn: string): string => arnName(arn);

/** A target ARN as a table shows it: what it is (`SNS topic`) and its name. */
export function targetParts(arn: string): { kind: string; name: string } {
  const [, , service = "", , , ...rest] = arn.split(":");
  const resource = rest.join(":");
  const name = (r: string) => r.split("/")[1] ?? r;
  switch (service) {
    case "sqs": return { kind: "SQS queue", name: resource };
    case "sns": return { kind: "SNS topic", name: resource };
    case "lambda": return { kind: "Lambda function", name: resource.replace(/^function:/, "").split(":")[0] ?? resource };
    case "logs": return { kind: "Log group", name: resource.replace(/^log-group:/, "").replace(/:\*$/, "") };
    case "kinesis": return { kind: "Kinesis stream", name: name(resource) };
    case "states": return { kind: "State machine", name: resource.replace(/^stateMachine:/, "") };
    case "events":
      return resource.startsWith("api-destination/") ? { kind: "API destination", name: name(resource) } : resource.startsWith("event-bus/") ? { kind: "Event bus", name: name(resource) } : { kind: "Target", name: arnName(arn) };
    default: return { kind: "Target", name: arnName(arn) };
  }
}

/** A target ARN as a person says it: `SNS topic orders-created`, `SQS queue pick-lists`, `Lambda function notify`. */
export const targetWords = (arn: string): string => {
  const p = targetParts(arn);
  return `${p.kind} ${p.name}`;
};

/** The line a rule's pattern is in a table: the sources and detail types it names (`larder.orders OrderPlaced`), the schedule of a schedule rule,
 * or `Custom pattern` for one that names neither. */
export function patternWords(pattern: string | undefined, schedule?: string): string {
  if (schedule) return schedule;
  if (!pattern) return "–";
  try {
    const p = JSON.parse(pattern) as Record<string, unknown>;
    const words = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
    const named = [...words(p.source), ...words(p["detail-type"])];
    return named.length > 0 ? named.join(" ") : "Custom pattern";
  } catch {
    return "Custom pattern";
  }
}

/** The pattern as indented JSON, when it is JSON. */
export function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

/** A pattern as the boards print it: objects over several lines, a list on one line (`"source": ["larder.orders"]`). */
export function prettyPattern(text: string): string {
  const inline = (v: unknown): string => (Array.isArray(v) ? `[${v.map(inline).join(", ")}]` : v !== null && typeof v === "object" ? `{ ${Object.entries(v).map(([k, x]) => `${JSON.stringify(k)}: ${inline(x)}`).join(", ")} }` : JSON.stringify(v));
  const show = (v: unknown, pad: string): string => {
    if (Array.isArray(v)) return inline(v);
    if (v !== null && typeof v === "object") {
      const rows = Object.entries(v).map(([k, x]) => `${pad}  ${JSON.stringify(k)}: ${show(x, `${pad}  `)}`);
      return rows.length === 0 ? "{}" : `{\n${rows.join(",\n")}\n${pad}}`;
    }
    return JSON.stringify(v);
  };
  try {
    return show(JSON.parse(text), "");
  } catch {
    return text;
  }
}

/** The event window of a replay, as the board writes it: `Nov 6 18:00 to Nov 7 18:00` (UTC, seconds since 1970). */
export function windowWords(startSeconds: number | undefined, endSeconds: number | undefined): string {
  if (startSeconds === undefined || endSeconds === undefined) return "–";
  const f = (s: number) => {
    const d = new Date(s * 1000);
    return `${d.toLocaleString("en-US", { month: "short", timeZone: "UTC" })} ${d.getUTCDate()} ${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`;
  };
  return `${f(startSeconds)} to ${f(endSeconds)}`;
}

/** `30 min`, `1 hour`, `24 hours`: the maximum event age of a retry policy. */
export function ageWords(seconds: number): string {
  if (seconds % 3600 === 0) return count(seconds / 3600, "hour");
  if (seconds % 60 === 0) return `${seconds / 60} min`;
  return `${seconds} s`;
}

/** `1 hour up to 20 attempts`, or `Default` for a target that sets no policy. */
export function retryWords(t: TargetItem): string {
  const r = t.RetryPolicy;
  if (!r) return "Default";
  const age = r.MaximumEventAgeInSeconds === undefined ? "24 hours" : ageWords(r.MaximumEventAgeInSeconds);
  const tries = r.MaximumRetryAttempts === undefined ? 185 : r.MaximumRetryAttempts;
  return `${age} up to ${count(tries, "attempt")}`;
}

/** What a target is sent: the whole event, a constant, a part of it, or an input transformer with its variables. */
export function inputWords(t: TargetItem): string {
  if (t.InputTransformer) {
    const names = Object.keys(t.InputTransformer.InputPathsMap ?? {});
    return `Input transformer${names.length > 0 ? ` ${names.join(", ")}` : ""}`;
  }
  if (t.InputPath) return `Part of the event ${t.InputPath}`;
  if (t.Input) return "Constant (JSON text)";
  return "Matched event";
}

const nameList = (names: readonly string[], most = 2): string => (names.length <= most ? list(names) : `${names.slice(0, most).join(", ")} and ${names.length - most} more`);

/** The line over the rules of a bus: how many, how many are on, how many targets, and where the first sends. */
export function rulesSentence(bus: string, rules: readonly RuleItem[], targets: Readonly<Record<string, readonly TargetItem[]>>): string {
  if (rules.length === 0) return `The ${bus} bus has no rules. Events sent to it are accepted and then match nothing.`;
  const on = rules.filter((r) => r.State === "ENABLED").length;
  const state = on === rules.length ? (rules.length === 1 ? "enabled" : rules.length === 2 ? "both enabled" : "all enabled") : on === 0 ? (rules.length === 1 ? "disabled" : "all disabled") : `${on} enabled`;
  const read = rules.filter((r) => targets[r.Name] !== undefined);
  const n = read.reduce((sum, r) => sum + (targets[r.Name]?.length ?? 0), 0);
  const head = `The ${bus} bus has ${count(rules.length, "rule")}${`, ${state}`}${read.length === rules.length ? `, with ${count(n, "target")}${rules.length > 1 ? " between them" : ""}` : ""}.`;
  const first = rules.find((r) => (targets[r.Name]?.length ?? 0) > 0);
  const to = first ? targets[first.Name]?.[0] : undefined;
  return first && to ? `${head} ${first.Name} sends to ${targetWords(to.Arn).replace(/^(\S+ \S+) /, "the $1 ")}.` : head;
}

/** The line over the archives: how many, how many events they hold, and which buses they take from. */
export function archivesSentence(archives: readonly ArchiveItem[], patterns: Readonly<Record<string, string | undefined>> = {}): string {
  if (archives.length === 0) return "";
  const events = archives.reduce((s, a) => s + (a.EventCount ?? 0), 0);
  const buses = [...new Set(archives.map((a) => busOfArn(a.EventSourceArn)))];
  const head = `${count(archives.length, "archive")} ${archives.length === 1 ? "holds" : "hold"} ${count(events, "event")} from ${nameList(buses)}.`;
  const patternOf = (a: ArchiveItem) => patterns[a.ArchiveName] ?? a.EventPattern;
  // The filtered archive that holds the most is the one worth naming.
  const filtered = [...archives].sort((a, b) => (b.EventCount ?? 0) - (a.EventCount ?? 0)).find((a) => patternOf(a));
  const what = filtered ? patternWords(patternOf(filtered)) : "";
  const named = what === "Custom pattern" || what === "–" ? "the events its pattern names" : `${what} events`;
  return filtered ? `${head} ${filtered.ArchiveName} keeps only ${named}${filtered.RetentionDays ? `, for ${retentionWords(filtered.RetentionDays)}` : ""}.` : head;
}

/** The line over the replays: how many, and what state they are in. */
export function replaysSentence(replays: readonly ReplayItem[]): string {
  if (replays.length === 0) return "";
  const states = new Map<string, number>();
  for (const r of replays) states.set(r.State ?? "", (states.get(r.State ?? "") ?? 0) + 1);
  const all = states.size === 1 ? `${[...states.keys()][0]!.toLowerCase()}` : "";
  const mixed = [...states].map(([s, n]) => `${n} ${s.toLowerCase()}`);
  return `${count(replays.length, "replay")}${all ? `, ${replays.length === 1 ? "" : "all "}${all}` : `: ${list(mixed)}`}.`;
}

/** The line over the API destinations: how many, how many are active, and the connection of the first. */
export function destsSentence(dests: readonly ApiDestItem[]): string {
  if (dests.length === 0) return "";
  const active = dests.filter((d) => d.ApiDestinationState === "ACTIVE").length;
  const state = active === dests.length ? (dests.length === 1 ? "active" : dests.length === 2 ? "both active" : "all active") : `${active} active`;
  const first = dests[0]!;
  return `${count(dests.length, "API destination")}, ${state}. ${first.Name} signs in through the ${connectionName(first.ConnectionArn)} connection.`;
}

/** The line over the buses: how many, and what each holds (its rules and archives, or that it has none). */
export function busesSentence(buses: readonly string[], rulesBy: Readonly<Record<string, readonly RuleItem[]>>, archives: readonly ArchiveItem[]): string {
  if (buses.length === 0) return "";
  const parts = [...buses].sort((a, b) => a.localeCompare(b)).flatMap((b) => {
    const rules = rulesBy[b];
    if (rules === undefined) return [];
    const kept = archives.filter((a) => busOfArn(a.EventSourceArn) === b).length;
    if (rules.length === 0) return [`${b} has no rules${kept > 0 ? ` and ${count(kept, "archive")}` : " yet"}`];
    if (rules.every((r) => r.ScheduleExpression)) return [`${b} runs the schedule ${rules.length === 1 ? "rule" : "rules"} ${list(rules.map((r) => r.Name))}`];
    return [`${b} has ${count(rules.length, "rule")}${kept > 0 ? ` and ${count(kept, "archive")}` : ""}`];
  });
  return `${count(buses.length, "event bus", "event buses")}.${parts.length > 0 ? ` ${parts.join("; ")}.` : ""}`;
}
