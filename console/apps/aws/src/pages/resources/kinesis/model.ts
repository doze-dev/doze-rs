// What the Kinesis pages say, as pure functions: a stream's row and fields, the sentences, the shard estimator, the rules of the forms' fields
// (the service's own limits), the sharing policy, and the requests the forms make.
import type { FieldValue } from "../../../api/types/FieldValue";
import type { ResourceRow } from "../../../api/types/ResourceRow";

/** `1 stream`, `3 streams`. */
export const count = (n: number, one: string, many = `${one}s`): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
/** `a`, `a and b`, `a, b and c`. */
export const list = (items: readonly string[]): string => (items.length <= 1 ? items.join("") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`);

/** `Provisioned`, `On-demand`. */
export const modeWords = (mode: string | null): string => (mode === "ON_DEMAND" ? "On-demand" : mode === "PROVISIONED" ? "Provisioned" : (mode ?? "–"));
/** `Active`, `Creating`. */
export const statusWords = (s: string | null): string => (s ? s.charAt(0) + s.slice(1).toLowerCase() : "–");

/** A stream as its list row says it (the descriptor's columns: status, capacity mode, created, consumers). */
export type StreamRow = { name: string; status: string | null; mode: string | null; created: string | null; consumers: number };
export const streamRows = (rows: readonly ResourceRow[]): StreamRow[] => rows.map((r) => ({ name: r.name, status: r.cells[0], mode: r.cells[1], created: r.cells[2], consumers: Number(r.cells[3] ?? 0) || 0 }));

/** The line over the list: how many, in which modes, and who consumes. `names` are the consumers' names by stream, as far as they are known. */
export function listSentence(rows: readonly StreamRow[], names: Readonly<Record<string, readonly string[]>>): string {
  if (rows.length === 0) return "";
  const prov = rows.filter((r) => r.mode === "PROVISIONED").length;
  const demand = rows.filter((r) => r.mode === "ON_DEMAND").length;
  const modes = [prov > 0 && `${prov} provisioned`, demand > 0 && `${demand} on-demand`].filter(Boolean) as string[];
  const head = `${count(rows.length, "data stream")}${modes.length > 0 ? `: ${list(modes)}` : ""}.`;
  const fed = rows.filter((r) => r.consumers > 0);
  const said = fed.slice(0, 2).map((r) => `${r.name} has ${count(r.consumers, "registered consumer")}${(names[r.name] ?? []).length > 0 ? `, ${list(names[r.name] ?? [])}` : ""}.`);
  const more = fed.length > 2 ? ` ${count(fed.length - 2, "other stream")} ${fed.length - 2 === 1 ? "has" : "have"} consumers too.` : "";
  return [head, ...said].join(" ") + more;
}

export const fieldOf = (fields: readonly FieldValue[], label: string): string | null => fields.find((f) => f.label === label)?.value ?? null;

/** A stream as its page says it. */
export type Stream = { name: string; arn: string; status: string | null; mode: string | null; shards: number; consumers: number; retention: number; encryption: string | null; kms: string | null; created: string | null };
export function streamOf(name: string, fields: readonly FieldValue[]): Stream {
  const n = (label: string, or = 0) => Number(fieldOf(fields, label) ?? or) || or;
  return { name, arn: fieldOf(fields, "ARN") ?? "", status: fieldOf(fields, "Status"), mode: fieldOf(fields, "Capacity mode"), shards: n("Open shards"), consumers: n("Consumers"), retention: n("Retention (hours)", 24), encryption: fieldOf(fields, "Encryption"), kms: fieldOf(fields, "KMS key"), created: fieldOf(fields, "Created") };
}

/** `24 hours`, `1 hour`. */
export const hoursWords = (h: number): string => `${h.toLocaleString("en-US")} ${h === 1 ? "hour" : "hours"}`;
/** `3 days`, `1 day 12 hours`, or nothing under a day. */
export function daysWords(h: number): string {
  const d = Math.floor(h / 24);
  const r = h % 24;
  if (d === 0) return "";
  return `${count(d, "day")}${r > 0 ? ` ${count(r, "hour")}` : ""}`;
}

/** The sentence of a stream's page: its shards, its retention, its consumers by name. */
export function streamSentence(s: Stream, consumers: readonly string[]): string {
  const base = `${s.name} has ${count(s.shards, "open shard")}, keeps records for ${hoursWords(s.retention)}`;
  if (s.consumers === 0) return `${base}, and has no registered consumer.`;
  return `${base}, and has ${count(s.consumers, "registered consumer")}${consumers.length > 0 ? `, ${list(consumers)}` : ""}${consumers.length === 1 ? ", which reads it with GetRecords" : ""}.`;
}

/** What a stream's shards give in AWS: each takes 1 MiB and 1,000 records a second in, and serves 2 MiB a second out. */
export function capacityWords(shards: number): string {
  return `${count(shards, "shard")} is ${shards.toLocaleString("en-US")} MiB per second and ${(shards * 1000).toLocaleString("en-US")} records per second in, ${(shards * 2).toLocaleString("en-US")} MiB per second out`;
}

/** The estimator of the Provisioned form: record size (KB, rounded up), records a second and consuming applications → shards. */
export type Estimate = { shards: number; formula: string };
export function estimate(sizeKb: number, perSecond: number, consumers: number): Estimate | null {
  if (!(sizeKb > 0 && perSecond > 0 && consumers >= 0) || ![sizeKb, perSecond, consumers].every(Number.isFinite)) return null;
  const write = Math.ceil(sizeKb) * perSecond;
  const read = write * Math.max(consumers, 0);
  const wShards = write / 1024;
  const rShards = read / 2048;
  const raw = Math.max(wShards, rShards);
  const fmt = (n: number) => n.toLocaleString("en-US");
  return { shards: Math.max(1, Math.ceil(raw)), formula: `max(${fmt(write)} / 1,024, ${fmt(read)} / 2,048) = ${(Math.round(raw * 100) / 100).toFixed(2)}, rounded up` };
}

// The rules of the forms' fields.

/** A stream's name: 1 to 128 of letters, digits, underscores, hyphens and periods. */
export function nameProblem(name: string): string | null {
  if (name === "") return "Enter a name.";
  if (name.length > 128) return "A name is 1 to 128 characters.";
  if (!/^[A-Za-z0-9_.-]+$/.test(name)) return "Use letters, digits, underscores, hyphens and periods only.";
  return null;
}

/** A shard count of a provisioned stream. */
export const shardsProblem = (text: string): string | null => (/^\d+$/.test(text.trim()) && Number(text) >= 1 ? (Number(text) > 500 ? "A stream here has up to 500 shards." : null) : "Enter a whole number of shards, 1 or more.");

/** A retention in hours: 24 to 8,760 (365 days). */
export const retentionProblem = (text: string): string | null => (/^\d+$/.test(text.trim()) && Number(text) >= 24 && Number(text) <= 8760 ? null : "Enter whole hours from 24 to 8,760 (365 days).");

export type Mode = "ON_DEMAND" | "PROVISIONED";
export type CreateForm = { name: string; mode: Mode; shards: string; tags: { key: string; value: string }[] };
export const emptyCreate: CreateForm = { name: "", mode: "ON_DEMAND", shards: "2", tags: [{ key: "", value: "" }] };

/** CreateStream's input: the mode always, the shard count for a provisioned stream, the tags that have a key. */
export function createInput(f: CreateForm): Record<string, unknown> {
  const tags = Object.fromEntries(f.tags.filter((t) => t.key.trim() !== "").map((t) => [t.key.trim(), t.value]));
  return { StreamName: f.name, ...(f.mode === "PROVISIONED" ? { ShardCount: Number(f.shards) || 0 } : {}), StreamModeDetails: { StreamMode: f.mode }, ...(Object.keys(tags).length > 0 ? { Tags: tags } : {}) };
}

/** The seven shard-level metrics, with what each counts. */
export const METRICS: { name: string; about: string }[] = [
  { name: "IncomingBytes", about: "Bytes successfully put to the shard." },
  { name: "IncomingRecords", about: "Records successfully put to the shard." },
  { name: "OutgoingBytes", about: "Bytes retrieved from the shard." },
  { name: "OutgoingRecords", about: "Records retrieved from the shard." },
  { name: "WriteProvisionedThroughputExceeded", about: "Records rejected due to throttling for the shard." },
  { name: "ReadProvisionedThroughputExceeded", about: "GetRecords calls throttled for the shard." },
  { name: "IteratorAgeMilliseconds", about: "Age of the last record in all GetRecords calls against the shard." },
];

/** What to enable and what to disable to get from `now` to `want` (the two calls of the metrics edit). */
export function metricsDiff(now: readonly string[], want: readonly string[]): { enable: string[]; disable: string[] } {
  return { enable: want.filter((m) => !now.includes(m)), disable: now.filter((m) => !want.includes(m)) };
}

/** The operation that takes the retention from `now` to `hours`: an increase or a decrease, chosen by which way it goes; null when it is the same. */
export function retentionOp(now: number, hours: number): "IncreaseStreamRetentionPeriod" | "DecreaseStreamRetentionPeriod" | null {
  return hours === now ? null : hours > now ? "IncreaseStreamRetentionPeriod" : "DecreaseStreamRetentionPeriod";
}

// The Data Viewer.

export const POSITIONS = [
  { value: "TRIM_HORIZON", label: "Trim horizon" },
  { value: "AT_SEQUENCE_NUMBER", label: "At sequence number" },
  { value: "AFTER_SEQUENCE_NUMBER", label: "After sequence number" },
  { value: "AT_TIMESTAMP", label: "At timestamp" },
  { value: "LATEST", label: "Latest" },
] as const;
export type Position = (typeof POSITIONS)[number]["value"];

/** GetShardIterator's input. */
export function iteratorInput(shard: string, position: Position, extra: { sequence: string; at: number | null }): Record<string, unknown> {
  return { ShardId: shard, ShardIteratorType: position, ...(position === "AT_SEQUENCE_NUMBER" || position === "AFTER_SEQUENCE_NUMBER" ? { StartingSequenceNumber: extra.sequence.trim() } : {}), ...(position === "AT_TIMESTAMP" && extra.at !== null ? { Timestamp: extra.at / 1000 } : {}) };
}

/** A record as GetRecords returns it. */
export type KRecord = { SequenceNumber: string; ApproximateArrivalTimestamp?: number; Data?: string; PartitionKey: string; EncryptionType?: string };

/** `49668410…698754`: the long sequence number cut to its first 8 and last 6 digits. */
export const shortSequence = (s: string): string => (s.length > 16 ? `${s.slice(0, 8)}…${s.slice(-6)}` : s);

/** A record's data, base64 on the wire, as text; `null` when it is not UTF-8 text. */
export function dataText(b64: string | undefined): string | null {
  if (!b64) return "";
  try {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/** The text indented, when it is a JSON object or array. */
export function prettyJson(text: string): string | null {
  const t = text.trim();
  if (!(t.startsWith("{") || t.startsWith("["))) return null;
  try {
    return JSON.stringify(JSON.parse(t), null, 2);
  } catch {
    return null;
  }
}

/** The arrival time of a record, in ms (GetRecords says seconds with a fraction). */
export const arrivalMs = (r: KRecord): number | null => (r.ApproximateArrivalTimestamp === undefined ? null : Math.round(r.ApproximateArrivalTimestamp * 1000));

// The sharing policy.

/** The actions the visual editor offers on a stream, all at the level of the data stream. */
export const SHARE_ACTIONS = ["kinesis:DescribeStreamSummary", "kinesis:ListShards", "kinesis:GetRecords", "kinesis:GetShardIterator", "kinesis:PutRecord", "kinesis:PutRecords"] as const;
/** The read actions, the ones the guide's example shares. */
export const READ_ACTIONS: readonly string[] = SHARE_ACTIONS.slice(0, 4);

/** A principal as a policy names it: an ARN as it is, an account ID as it is (`"AWS": "123456789012"` is valid). */
export const principalProblem = (p: string): string | null => (/^(\d{12}|arn:aws:iam::\d{12}:(root|user\/[\w+=,.@/-]+|role\/[\w+=,.@/-]+))$/.test(p.trim()) ? null : "Give an account ID, an IAM user ARN or an IAM role ARN.");

/** The policy the visual editor writes, in the shape of the guide's read-access example. */
export function sharePolicy(principal: string, actions: readonly string[], streamArn: string): string {
  return JSON.stringify({ Version: "2012-10-17", Id: "__default_sharedthroughput_read_policy_ID", Statement: [{ Sid: "sharedthroughputreadstatement", Effect: "Allow", Principal: { AWS: principal }, Action: [...actions], Resource: streamArn }] }, null, 2);
}

/** The statements of a policy as the page lists them: effect, principals, actions; `null` when it is not a policy. */
export type Statement = { sid: string; effect: string; principals: string[]; actions: string[] };
export function statementsOf(policy: string): Statement[] | null {
  try {
    const p = JSON.parse(policy) as { Statement?: unknown };
    const raw = Array.isArray(p.Statement) ? p.Statement : p.Statement ? [p.Statement] : [];
    const many = (v: unknown): string[] => (typeof v === "string" ? [v] : Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
    return raw.map((s: Record<string, unknown>) => {
      const pr = s.Principal;
      const principals = typeof pr === "string" ? [pr] : pr && typeof pr === "object" ? Object.values(pr as Record<string, unknown>).flatMap(many) : [];
      return { sid: typeof s.Sid === "string" ? s.Sid : "", effect: typeof s.Effect === "string" ? s.Effect : "", principals, actions: many(s.Action) };
    });
  } catch {
    return null;
  }
}

/** Whether the service answered "no policy": it answers `{}` for a stream with none. */
export const noPolicy = (policy: string | undefined): boolean => policy === undefined || policy.trim() === "" || policy.trim() === "{}";
