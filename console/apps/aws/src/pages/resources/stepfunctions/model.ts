// What the Step Functions pages know of a machine's definition and of what they say about it: the words for a Task's resource
// (`DynamoDB GetItem`), what a machine is wired to, the role the studio makes and the permissions it gives it, the names the
// service accepts. Pure, so it is tested without a browser.
import type { AslState, Definition } from "./graph";
import { statesOf } from "./graph";

/** A name StartExecution, CreateStateMachine and CreateActivity take: 1 to 80 characters, none of the ones AWS refuses. */
export const NAME = /^[^\s<>{}[\]?*"#%\\^|~`$&,;:/]{1,80}$/;
/** What a name may be made of for its logs to go to CloudWatch. */
export const LOG_NAME = /^[0-9A-Za-z\-_.]+$/;

/** Why a state machine or activity name is not one, in the board's words; `null` when it is. */
export function nameProblem(name: string, taken: readonly string[] = []): string | null {
  if (name === "") return "Enter a name.";
  if (name.length > 80) return "A name is at most 80 characters.";
  if (!NAME.test(name)) return "A name has no whitespace, wildcard characters ( ? * ), brackets ( < > { } [ ] ) or special characters ( \" # % \\ ^ | ~ ` $ & , ; : / ).";
  if (taken.includes(name)) return `${name} already exists in this Region.`;
  return null;
}

/** `order-fulfilment and thumbnail-batch already exist`, as the hint says it. */
export const existingWords = (names: readonly string[]): string => {
  if (names.length === 0) return "";
  const list = names.length === 1 ? names[0] : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  return `${list} already ${names.length === 1 ? "exists" : "exist"} in this Region.`;
};

/** The service's word for each task resource the console names: `arn:aws:states:::sqs:sendMessage` is SQS SendMessage. */
const SERVICE_WORDS: Record<string, { title: string; iam: string }> = {
  lambda: { title: "Lambda", iam: "lambda" },
  sqs: { title: "SQS", iam: "sqs" },
  sns: { title: "SNS", iam: "sns" },
  dynamodb: { title: "DynamoDB", iam: "dynamodb" },
  events: { title: "EventBridge", iam: "events" },
  states: { title: "Step Functions", iam: "states" },
  s3: { title: "S3", iam: "s3" },
  secretsmanager: { title: "Secrets Manager", iam: "secretsmanager" },
  ssm: { title: "Systems Manager", iam: "ssm" },
  kinesis: { title: "Kinesis", iam: "kinesis" },
  logs: { title: "CloudWatch Logs", iam: "logs" },
};

const upper = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** A Task's resource read: the service, the action as the service names it, and a pattern suffix (`.sync`, `.waitForTaskToken`). */
export type TaskResource = { service: string; title: string; action: string; pattern: string; activity?: string };

export function taskResource(resource: string | undefined): TaskResource | null {
  if (!resource) return null;
  const activity = /^arn:aws:states:[^:]*:[^:]*:activity:(.+)$/.exec(resource);
  if (activity) return { service: "states", title: "Activity", action: activity[1] ?? "", pattern: "", activity: activity[1] };
  const fn = /^arn:aws:lambda:[^:]*:[^:]*:function:([^:]+)/.exec(resource);
  if (fn) return { service: "lambda", title: "Lambda", action: "Invoke", pattern: "" };
  const m = /^arn:aws:states:::(?:aws-sdk:)?([a-z0-9]+):([A-Za-z0-9]+)(\.[A-Za-z]+)?$/.exec(resource);
  if (!m) return null;
  const service = m[1] ?? "";
  return { service, title: SERVICE_WORDS[service]?.title ?? service, action: upper(m[2] ?? ""), pattern: m[3] ?? "" };
}

/** `DynamoDB GetItem`, `Activity pack-order`: what a Task state is, by its resource; the state's type for anything else. */
export function taskWords(state: Pick<AslState, "Type" | "Resource">): string {
  if (state.Type !== "Task") return state.Type ?? "";
  const t = taskResource(state.Resource);
  if (!t) return "Task";
  return t.activity ? `Activity ${t.activity}` : `${t.title} ${t.action}`;
}

const asText = (v: unknown): string => (typeof v === "string" ? v : "");
const tailOf = (arn: string): string => arn.split(/[:/]/).filter(Boolean).pop() ?? arn;

/** What a Task acts on, by name: the table, queue, topic, function or bucket in its parameters (`orders-table`). */
export function taskTarget(state: AslState): string | null {
  const t = taskResource(state.Resource);
  if (!t || t.activity) return t?.activity ?? null;
  const p = state.Parameters ?? {};
  const name =
    t.service === "lambda" ? asText(p.FunctionName) || lambdaFromResource(state.Resource)
    : t.service === "sqs" ? tailOf(asText(p.QueueUrl))
    : t.service === "sns" ? tailOf(asText(p.TopicArn))
    : t.service === "dynamodb" ? asText(p.TableName)
    : t.service === "s3" ? asText(p.Bucket)
    : t.service === "states" ? tailOf(asText(p.StateMachineArn))
    : "";
  return name ? tailOf(name) : null;
}

const lambdaFromResource = (resource: string | undefined): string => /:function:([^:]+)/.exec(resource ?? "")?.[1] ?? "";

/** Every state of a definition, in the branches and the item processor too, by name. */
export function allStates(def: Definition | undefined): [string, AslState][] {
  const out: [string, AslState][] = [];
  for (const [name, s] of Object.entries(statesOf(def))) {
    out.push([name, s]);
    for (const b of s.Branches ?? []) out.push(...allStates(b));
    if (s.ItemProcessor) out.push(...allStates(s.ItemProcessor));
    if (s.Iterator) out.push(...allStates(s.Iterator));
  }
  return out;
}

/** What a resource of the console a Task names is: its service, kind and name. */
export type Wire = { service: string; kind: string; name: string };

/** The resources a definition's Tasks act on, once each in the order the states name them: `orders-table`, `pick-lists`. */
export function wiresOf(def: Definition | undefined): Wire[] {
  const out: Wire[] = [];
  const seen = new Set<string>();
  for (const [, s] of allStates(def)) {
    const t = taskResource(s.Resource);
    const name = taskTarget(s);
    if (!t || !name) continue;
    const kind = t.activity ? ["stepfunctions", "activities"] : ({ lambda: ["lambda", "functions"], sqs: ["sqs", "queues"], sns: ["sns", "topics"], dynamodb: ["dynamodb", "tables"], s3: ["s3", "buckets"], states: ["stepfunctions", "machines"] } as Record<string, string[]>)[t.service];
    if (!kind || seen.has(`${kind[0]}/${name}`)) continue;
    seen.add(`${kind[0]}/${name}`);
    out.push({ service: kind[0] ?? "", kind: kind[1] ?? "", name });
  }
  return out;
}

/** The role the studio makes for a machine. */
export const roleNameOf = (machine: string): string => `StepFunctions-${machine}-role`.slice(0, 64);

/** The trust policy of that role. */
export const TRUST = { Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "states.amazonaws.com" }, Action: "sts:AssumeRole" }] };

const ACTIONS: Record<string, (action: string) => string> = {
  lambda: () => "lambda:InvokeFunction",
  sqs: (a) => `sqs:${a}`,
  sns: (a) => `sns:${a}`,
  dynamodb: (a) => `dynamodb:${a}`,
  events: (a) => `events:${a}`,
  states: (a) => `states:${a}`,
  s3: (a) => `s3:${a}`,
  secretsmanager: (a) => `secretsmanager:${a}`,
  ssm: (a) => `ssm:${a}`,
  kinesis: (a) => `kinesis:${a}`,
};

/** The permissions the role gets: what each Task of the definition calls, on what it names, and nothing else. `null` for a
 * definition with no Task that calls a service (the role then only trusts Step Functions). */
export function permissionsOf(def: Definition | undefined, where: { region: string; account: string }): { Version: string; Statement: { Effect: "Allow"; Action: string[]; Resource: string }[] } | null {
  const grouped = new Map<string, Set<string>>();
  for (const [, s] of allStates(def)) {
    const t = taskResource(s.Resource);
    if (!t || t.activity || !ACTIONS[t.service]) continue;
    const name = taskTarget(s);
    const resource = resourceArn(t.service, name, where);
    const set = grouped.get(resource) ?? new Set<string>();
    set.add(ACTIONS[t.service]?.(t.action) ?? "");
    grouped.set(resource, set);
  }
  if (grouped.size === 0) return null;
  return { Version: "2012-10-17", Statement: [...grouped].map(([Resource, a]) => ({ Effect: "Allow" as const, Action: [...a].sort(), Resource })) };
}

function resourceArn(service: string, name: string | null, w: { region: string; account: string }): string {
  if (!name) return "*";
  switch (service) {
    case "lambda": return `arn:aws:lambda:${w.region}:${w.account}:function:${name}`;
    case "sqs": return `arn:aws:sqs:${w.region}:${w.account}:${name}`;
    case "sns": return `arn:aws:sns:${w.region}:${w.account}:${name}`;
    case "dynamodb": return `arn:aws:dynamodb:${w.region}:${w.account}:table/${name}`;
    case "s3": return `arn:aws:s3:::${name}/*`;
    case "states": return `arn:aws:states:${w.region}:${w.account}:stateMachine:${name}`;
    default: return "*";
  }
}

/** The log group Step Functions names for a machine's logs by default. */
export const logGroupOf = (machine: string): string => `/aws/vendedlogs/states/${machine}-Logs`;
export const logGroupArn = (machine: string, w: { region: string; account: string }): string => `arn:aws:logs:${w.region}:${w.account}:log-group:${logGroupOf(machine)}:*`;

/** The text of a definition a person writes: two spaces, as the console shows it. */
export const pretty = (def: unknown): string => JSON.stringify(def, null, 2);

/** `Version 2, 100%` / `Version 2, 90% · Version 3, 10%`: an alias's routing, said. */
export function routingWords(routes: readonly { stateMachineVersionArn: string; weight: number }[] | undefined): string {
  if (!routes || routes.length === 0) return "–";
  return routes.map((r) => `Version ${r.stateMachineVersionArn.split(":").pop()}, ${r.weight}%`).join(" · ");
}

/** The number of a version's ARN (`…:order-fulfilment:3` is `3`), or `null` for an ARN that is not a version's. */
export function versionOf(arn: string | undefined | null): string | null {
  if (!arn) return null;
  const last = arn.split(":").pop() ?? "";
  return /^\d+$/.test(last) ? last : null;
}

/** The alias of an alias ARN (`…:order-fulfilment:PROD`), or `null`. */
export function aliasOf(arn: string | undefined | null): string | null {
  if (!arn) return null;
  const parts = arn.split(":");
  return parts.length >= 8 && !/^\d+$/.test(parts[parts.length - 1] ?? "") ? (parts[parts.length - 1] ?? null) : null;
}

/** An alias name the service accepts: 1 to 80 of letters, digits, `-`, `_`, `.`; not an integer. `null` when it is one. */
export function aliasProblem(name: string, taken: readonly string[]): string | null {
  if (name === "") return "Enter a name.";
  if (name.length > 80) return "An alias name is at most 80 characters.";
  if (!/^[0-9A-Za-z\-_.]+$/.test(name)) return "An alias name is letters, digits, - _ . only.";
  if (/^\d+$/.test(name)) return "An alias name is not an integer: it would read as a version.";
  if (taken.includes(name)) return `${name} already exists.`;
  return null;
}

/** What an Express machine does with its runs, from its logging level (`undefined` when the page has not read it): it keeps no
 * execution history, and writes events to a log group only while its logging is on (the level OFF, the default, writes nothing). */
export function expressWords(level?: string): string {
  const l = level?.toUpperCase();
  if (l === undefined) return "it keeps no execution history: its runs are written to a log group only while its logging is on.";
  if (l === "OFF") return "it keeps no execution history, and its logging is Off, so its runs are recorded nowhere. Turn logging on to write them to a log group.";
  if (l === "ALL") return "it keeps no execution history: its runs are written to its log group.";
  return `it keeps no execution history: only the events that report a failure are written to its log group (logging ${l}).`;
}

/** `order-fulfilment has 2 executions running. thumbnail-batch is Express, so it keeps no execution history: its runs are written
 * to a log group only while its logging is on.` What the list says of its machines, from how many executions of each are running. */
export function listSentence(machines: readonly { name: string; express: boolean }[], running: Readonly<Record<string, number>>): string | null {
  if (machines.length === 0) return null;
  const busy = machines.filter((m) => (running[m.name] ?? 0) > 0);
  const parts = busy.map((m) => `${m.name} has ${running[m.name]} ${running[m.name] === 1 ? "execution" : "executions"} running.`);
  if (parts.length === 0) parts.push(machines.every((m) => m.express) ? "" : "No execution is running.");
  const express = machines.filter((m) => m.express).map((m) => m.name);
  if (express.length > 0) {
    const names = express.length === 1 ? express[0] : `${express.slice(0, -1).join(", ")} and ${express[express.length - 1]}`;
    parts.push(express.length === 1 ? `${names} is Express, so ${expressWords()}` : `${names} are Express, so they keep no execution history: their runs are written to a log group only while their logging is on.`);
  }
  return parts.filter(Boolean).join(" ");
}

/** The line under a machine's name: what is running, and the last to fail and where it stopped. */
export function machineSentence(name: string, express: boolean, runs: { running: number; never: boolean; failed: { name: string; error?: string; state?: string } | null } | undefined, startAt?: string, level?: string): string | null {
  if (express) return `${name} is Express, so ${expressWords(level)}`;
  if (!runs) return null;
  if (runs.never) return `${name} has never run. Start an execution with a JSON input${startAt ? `; it begins at ${startAt}` : ""}.`;
  const running = runs.running === 0 ? `${name} has no execution running.` : `${name} has ${runs.running} ${runs.running === 1 ? "execution" : "executions"} running.`;
  const f = runs.failed;
  if (!f) return running;
  return `${running} The last to fail, ${f.name},${f.state ? ` stopped at ${f.state}` : " stopped"}${f.error ? ` with ${f.error}` : ""}.`;
}

const spanWords = (s: number): string => {
  if (s % 86400 === 0) return `${s / 86400} ${s === 86400 ? "day" : "days"}`;
  if (s % 3600 === 0) return s === 3600 ? "an hour" : `${s / 3600} hours`;
  if (s % 60 === 0) return s === 60 ? "a minute" : `${s / 60} minutes`;
  return `${s} seconds`;
};

/** What a definition teaches about its timeouts: the size limit, the machine's own timeout, and the Tasks that set a heartbeat. */
export function definitionNotes(def: Definition | undefined): { text: string; names: string[] } {
  const parts = ["A definition may be up to 1,048,576 bytes."];
  const names: string[] = [];
  if (typeof def?.TimeoutSeconds === "number") parts.push(`TimeoutSeconds: ${def.TimeoutSeconds} ends any execution still open after ${spanWords(def.TimeoutSeconds)} as Timed out.`);
  for (const [name, s] of allStates(def)) {
    if (typeof s.HeartbeatSeconds !== "number") continue;
    names.push(name);
    parts.push(`${name} sets HeartbeatSeconds ${s.HeartbeatSeconds} and ${typeof s.TimeoutSeconds === "number" ? `TimeoutSeconds ${s.TimeoutSeconds}` : "no TimeoutSeconds of its own"}.`);
  }
  return { text: parts.join(" "), names };
}

/** What a new execution of a machine is started with: `{}`, or a skeleton of the input its first state reads (`$.order_id` in its
 * Parameters or its InputPath, a Choice's Variable), each member empty, so the first Start is not a run that fails at once on a
 * path the input does not have. A state's ResultPath, OutputPath and ResultSelector write, they do not read, and `$$.` is the
 * context object: neither is the input's. */
export function inputSkeleton(def: Definition | null | undefined): string {
  const start = def?.States?.[def.StartAt ?? ""];
  if (!start) return "{}";
  const paths = new Set<string>();
  const scan = (v: unknown): void => {
    if (typeof v === "string") {
      for (const m of v.matchAll(/(?<![$\w])\$\.([A-Za-z_][\w-]*(?:\.[A-Za-z_][\w-]*)*)/g)) paths.add(m[1] as string);
    } else if (Array.isArray(v)) v.forEach(scan);
    else if (v !== null && typeof v === "object") {
      for (const [k, x] of Object.entries(v)) if (k !== "ResultPath" && k !== "OutputPath" && k !== "ResultSelector") scan(x);
    }
  };
  scan(start);
  if (paths.size === 0) return "{}";
  const input: Record<string, unknown> = {};
  for (const p of [...paths].sort()) {
    const keys = p.split(".");
    let at = input;
    keys.forEach((k, i) => {
      if (i === keys.length - 1) {
        if (!(k in at)) at[k] = "";
      } else {
        const next = at[k];
        if (typeof next === "object" && next !== null) at = next as Record<string, unknown>;
        else if (!(k in at) || next === "") {
          const made: Record<string, unknown> = {};
          at[k] = made;
          at = made;
        }
      }
    });
  }
  return JSON.stringify(input, null, 2);
}
