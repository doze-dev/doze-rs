// What the function's forms send: each form's rules in Lambda's own numbers, and the input it makes of what was typed (only what
// changed, where the call is an update). Pure.
import type { Fn, UrlCors } from "./model";

export type Problems = Record<string, string>;

const whole = (s: string): number | null => (/^\d+$/.test(s.trim()) ? Number(s) : null);

// --- general configuration -------------------------------------------------------------------------------------------
export type General = { description: string; memory: string; ephemeral: string; timeout: string; role: string };

export const generalOf = (f: Fn): General => ({ description: f.description, memory: String(f.memory), ephemeral: String(f.ephemeral), timeout: String(f.timeout), role: f.role });

export function generalProblems(g: General): Problems {
  const p: Problems = {};
  if (g.description.length > 256) p.description = "A description is at most 256 characters.";
  const memory = whole(g.memory);
  if (memory === null || memory < 128 || memory > 10240) p.memory = "Memory is a whole number from 128 to 10,240 MB.";
  const ephemeral = whole(g.ephemeral);
  if (ephemeral === null || ephemeral < 512 || ephemeral > 10240) p.ephemeral = "Ephemeral storage is a whole number from 512 to 10,240 MB.";
  const timeout = whole(g.timeout);
  if (timeout === null || timeout < 1 || timeout > 900) p.timeout = "A timeout is a whole number of seconds from 1 to 900.";
  if (!g.role.startsWith("arn:")) p.role = "Choose the role the function runs as.";
  return p;
}

/** UpdateFunctionConfiguration's members for what changed; empty when nothing did. */
export function generalChange(f: Fn, g: General): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  if (g.description !== f.description) out.Description = g.description;
  if (Number(g.memory) !== f.memory) out.MemorySize = Number(g.memory);
  if (Number(g.ephemeral) !== f.ephemeral) out.EphemeralStorage = { Size: Number(g.ephemeral) };
  if (Number(g.timeout) !== f.timeout) out.Timeout = Number(g.timeout);
  if (g.role !== f.role) out.Role = g.role;
  return out;
}

// --- environment variables -------------------------------------------------------------------------------------------
/** A key AWS takes: a letter, then letters, digits and underscores, two characters at least. */
export const ENV_KEY = /^[a-zA-Z][a-zA-Z0-9_]+$/;
/** The variables Lambda sets itself and refuses on a function. */
export const RESERVED_KEYS = ["_HANDLER", "AWS_REGION", "AWS_EXECUTION_ENV", "AWS_LAMBDA_FUNCTION_NAME", "AWS_LAMBDA_FUNCTION_MEMORY_SIZE", "AWS_LAMBDA_FUNCTION_VERSION", "AWS_LAMBDA_INITIALIZATION_TYPE", "AWS_LAMBDA_LOG_GROUP_NAME", "AWS_LAMBDA_LOG_STREAM_NAME", "AWS_ACCESS_KEY", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "AWS_LAMBDA_RUNTIME_API", "LAMBDA_TASK_ROOT", "LAMBDA_RUNTIME_DIR"];
/** All keys and values together may be at most 4 KB. */
export const ENV_MOST = 4096;

/** What is wrong with the rows, by their position: a key that is not a key, a reserved one, and the 4 KB total (under `""`). */
export function envProblems(rows: readonly { key: string; value: string }[]): Problems {
  const p: Problems = {};
  rows.forEach((r, i) => {
    if (r.key === "" && r.value === "") return;
    if (!ENV_KEY.test(r.key)) p[String(i)] = "A key starts with a letter, has at least two characters, and holds only letters, digits and underscores.";
    else if (RESERVED_KEYS.includes(r.key)) p[String(i)] = `${r.key} is a variable Lambda sets itself: it is refused on a function.`;
  });
  const size = rows.reduce((n, r) => n + new TextEncoder().encode(r.key + r.value).length, 0);
  if (size > ENV_MOST) p[""] = `All keys and values together are ${size} bytes: at most 4 KB (4,096 bytes).`;
  return p;
}

/** The whole map, which an update replaces. */
export function envInput(rows: readonly { key: string; value: string }[]): { Environment: { Variables: Record<string, string> } } {
  return { Environment: { Variables: Object.fromEntries(rows.filter((r) => r.key !== "" || r.value !== "").map((r) => [r.key, r.value])) } };
}

// --- a trigger: an SQS event source mapping --------------------------------------------------------------------------
export type Trigger = { queue: string | null; enabled: boolean; batch: string };

/** An SQS queue holds at most 10,000 records in a batch for a standard queue and 10 for a FIFO queue. */
export const batchMost = (queueArn: string | null): number => (queueArn?.endsWith(".fifo") ? 10 : 10_000);

export function triggerProblems(t: Trigger, create: boolean): Problems {
  const p: Problems = {};
  if (create && !t.queue) p.queue = "Choose the queue the function reads from.";
  const n = whole(t.batch);
  if (n === null || n < 1 || n > batchMost(t.queue)) p.batch = `A batch is from 1 to ${batchMost(t.queue).toLocaleString("en-US")} records.`;
  return p;
}

export const mappingCreate = (name: string, t: Trigger) => ({ EventSourceArn: t.queue ?? "", FunctionName: name, Enabled: t.enabled, BatchSize: Number(t.batch) });

/** UpdateEventSourceMapping's members for what changed. */
export function mappingUpdate(uuid: string, from: { enabled: boolean; batch: number }, t: Trigger): Record<string, unknown> {
  const out: Record<string, unknown> = { UUID: uuid };
  if (t.enabled !== from.enabled) out.Enabled = t.enabled;
  if (Number(t.batch) !== from.batch) out.BatchSize = Number(t.batch);
  return out;
}

/** What SQS must hide a message for, at least: six times the function's timeout (Lambda's advice). */
export const visibilityFor = (timeout: number): number => timeout * 6;

// --- destinations of asynchronous invocations ------------------------------------------------------------------------
export type Destination = { condition: "OnFailure" | "OnSuccess"; type: "sqs" | "sns" | "lambda"; target: string | null };

export function destinationProblems(d: Destination): Problems {
  const p: Problems = {};
  if (!d.target) p.target = "Choose where the record goes.";
  else if (d.target.endsWith(".fifo")) p.target = "A FIFO queue or topic cannot be a destination: only standard ones work.";
  return p;
}

export const destinationInput = (name: string, d: Destination) => ({ FunctionName: name, DestinationConfig: { [d.condition]: { Destination: d.target ?? "" } } });

// --- a permission (the resource-based policy) ------------------------------------------------------------------------
export type Permission = { sid: string; principal: string; sourceArn: string; sourceAccount: string; action: string };

export function permissionProblems(p: Permission, taken: readonly string[]): Problems {
  const out: Problems = {};
  if (!/^[a-zA-Z0-9_-]{1,100}$/.test(p.sid)) out.sid = "A statement ID is 1 to 100 letters, digits, hyphens and underscores.";
  else if (taken.includes(p.sid)) out.sid = `${p.sid} is taken: the policy already has a statement with that ID.`;
  if (p.principal.trim() === "") out.principal = "Name the service, account, user or role that may call the function.";
  return out;
}

export function permissionInput(name: string, p: Permission) {
  const out: Record<string, unknown> = { FunctionName: name, StatementId: p.sid, Action: p.action, Principal: p.principal.trim() };
  if (p.sourceArn.trim()) out.SourceArn = p.sourceArn.trim();
  if (p.sourceAccount.trim()) out.SourceAccount = p.sourceAccount.trim();
  return out;
}

/** What a principal given as a service most likely names a source of (`s3.amazonaws.com` is a bucket). */
export function sourceHint(principal: string, fn: string): string | null {
  if (principal === "s3.amazonaws.com") return "For an AWS service, the ARN of the resource that invokes the function: for S3, a bucket such as arn:aws:s3:::receipts.";
  if (principal === "sns.amazonaws.com") return "For an AWS service, the ARN of the resource that invokes the function: for SNS, the topic's ARN.";
  if (principal === "events.amazonaws.com") return "For an AWS service, the ARN of the resource that invokes the function: for EventBridge, the rule's ARN.";
  return principal.endsWith(".amazonaws.com") ? `The ARN of the ${principal.split(".")[0]} resource that invokes ${fn}.` : null;
}

// --- a function URL --------------------------------------------------------------------------------------------------
export type UrlForm = { auth: "AWS_IAM" | "NONE"; origins: string; methods: string; headers: string; maxAge: string };

export function urlProblems(u: UrlForm): Problems {
  const p: Problems = {};
  if (u.maxAge.trim() !== "") {
    const n = whole(u.maxAge);
    if (n === null || n > 86_400) p.maxAge = "MaxAge is a whole number of seconds, 0 to 86,400.";
  }
  return p;
}

const split = (s: string): string[] => s.split(",").map((x) => x.trim()).filter(Boolean);

/** The Cors member, or none when nothing was given. */
export function corsOf(u: UrlForm): UrlCors | null {
  const out: UrlCors = {};
  if (split(u.origins).length) out.AllowOrigins = split(u.origins);
  if (split(u.methods).length) out.AllowMethods = split(u.methods);
  if (split(u.headers).length) out.AllowHeaders = split(u.headers);
  if (u.maxAge.trim() !== "") out.MaxAge = Number(u.maxAge);
  return Object.keys(out).length ? out : null;
}

export function urlInput(name: string, u: UrlForm) {
  const cors = corsOf(u);
  return { FunctionName: name, AuthType: u.auth, ...(cors ? { Cors: cors } : {}) };
}

// --- versions and aliases --------------------------------------------------------------------------------------------
export type AliasForm = { name: string; description: string; version: string | null };

export function aliasProblems(a: AliasForm, taken: readonly string[]): Problems {
  const p: Problems = {};
  if (!/^(?!^[0-9]+$)([a-zA-Z0-9-_]+)$/.test(a.name)) p.name = "An alias name is letters, digits, hyphens and underscores, and not only digits.";
  else if (taken.includes(a.name)) p.name = `${a.name} already exists: an alias name is used once.`;
  if (a.description.length > 256) p.description = "A description is at most 256 characters.";
  if (!a.version) p.version = "Choose the version the alias points to.";
  return p;
}

export const aliasInput = (fn: string, a: AliasForm) => ({ FunctionName: fn, Name: a.name, FunctionVersion: a.version ?? "", ...(a.description ? { Description: a.description } : {}) });

/** The number the next published version will have: Lambda only goes up. */
export const nextVersion = (versions: readonly { Version?: string }[]): number => Math.max(0, ...versions.map((v) => Number(v.Version)).filter((n) => Number.isFinite(n))) + 1;

// --- layers ----------------------------------------------------------------------------------------------------------
export type LayerForm = { name: string; description: string; license: string; archs: string[]; runtimes: string[] };

export function layerProblems(l: LayerForm, hasFile: boolean): Problems {
  const p: Problems = {};
  if (!/^[a-zA-Z0-9-_]{1,64}$/.test(l.name)) p.name = "A layer name is 1 to 64 letters, digits, hyphens and underscores.";
  if (l.description.length > 256) p.description = "A description is at most 256 characters.";
  if (!hasFile) p.file = "Choose the .zip file the layer is made of.";
  if (l.license.length > 512) p.license = "A license is at most 512 characters.";
  if (l.runtimes.length > 15) p.runtimes = "A layer names at most 15 runtimes.";
  return p;
}

export function layerInput(l: LayerForm, zipBase64: string) {
  return { LayerName: l.name, Content: { ZipFile: zipBase64 }, ...(l.description ? { Description: l.description } : {}), ...(l.archs.length ? { CompatibleArchitectures: l.archs } : {}), ...(l.runtimes.length ? { CompatibleRuntimes: l.runtimes } : {}), ...(l.license ? { LicenseInfo: l.license } : {}) };
}

/** A function takes at most five layers. */
export const LAYERS_MOST = 5;

/** The ARN of a layer version. */
export const layerArn = (region: string, account: string, name: string, version: number): string => `arn:aws:lambda:${region}:${account}:layer:${name}:${version}`;

// --- a function --------------------------------------------------------------------------------------------------
export type Create = { name: string; runtime: string; arch: "x86_64" | "arm64"; role: string | null };

export function createProblems(c: Create, taken: readonly string[]): Problems {
  const p: Problems = {};
  if (!/^[a-zA-Z0-9-_]{1,64}$/.test(c.name)) p.name = "A function name is 1 to 64 letters, digits, hyphens and underscores.";
  else if (taken.includes(c.name)) p.name = `${c.name} already exists in this Region.`;
  if (!c.role) p.role = "Choose the execution role: CreateFunction requires it.";
  return p;
}
