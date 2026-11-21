// What a function's pages know of it, read from the console's own answers: its runtime in the console's words, its settings, the
// things that trigger it, the sentence under its name. Pure, so it is tested without a browser.
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { Wired } from "../../../api/types/Wired";

/** The runtimes a function can be made with here (the ones doze-aws runs: a Python, a Node.js and a Ruby on this machine). */
export const RUNTIMES: { id: string; label: string; family: Family }[] = [
  { id: "python3.13", label: "Python 3.13", family: "python" },
  { id: "python3.12", label: "Python 3.12", family: "python" },
  { id: "python3.11", label: "Python 3.11", family: "python" },
  { id: "python3.10", label: "Python 3.10", family: "python" },
  { id: "nodejs22.x", label: "Node.js 22.x", family: "node" },
  { id: "nodejs20.x", label: "Node.js 20.x", family: "node" },
  { id: "ruby3.4", label: "Ruby 3.4", family: "ruby" },
  { id: "ruby3.3", label: "Ruby 3.3", family: "ruby" },
];

export type Family = "python" | "node" | "ruby";

/** `python3.13` as `Python 3.13`, `nodejs22.x` as `Node.js 22.x`; a runtime the console does not know as it is. */
export function runtimeName(id: string | null | undefined): string {
  if (!id) return "";
  const known = RUNTIMES.find((r) => r.id === id);
  if (known) return known.label;
  const m = /^(python|ruby|java|dotnet|nodejs)([\d.]+x?)$/.exec(id);
  if (!m) return id === "provided.al2023" ? "Amazon Linux 2023" : id === "provided.al2" ? "Amazon Linux 2" : id;
  return `${{ python: "Python", ruby: "Ruby", java: "Java", dotnet: ".NET", nodejs: "Node.js" }[m[1] ?? ""]} ${m[2]}`;
}

/** The program on this machine that runs a runtime's code, by name; `null` for a runtime that brings its own (`provided`). */
export function interpreterOf(runtime: string | null | undefined): string | null {
  if (!runtime) return null;
  if (runtime.startsWith("python")) return "python3";
  if (runtime.startsWith("nodejs")) return "node";
  if (runtime.startsWith("ruby")) return "ruby";
  return null;
}

/** The Hello world example a new function is made from: its one file and the handler that names it. */
export const HELLO: Record<Family, { file: string; handler: string; source: string }> = {
  python: { file: "lambda_function.py", handler: "lambda_function.lambda_handler", source: "import json\n\ndef lambda_handler(event, context):\n    # TODO implement\n    return {\n        'statusCode': 200,\n        'body': json.dumps('Hello from Lambda!')\n    }\n" },
  node: { file: "index.mjs", handler: "index.handler", source: "export const handler = async (event) => {\n  // TODO implement\n  const response = {\n    statusCode: 200,\n    body: JSON.stringify('Hello from Lambda!'),\n  };\n  return response;\n};\n" },
  ruby: { file: "lambda_function.rb", handler: "lambda_function.lambda_handler", source: "require 'json'\n\ndef lambda_handler(event:, context:)\n    # TODO implement\n    { statusCode: 200, body: JSON.generate('Hello from Lambda!') }\nend\n" },
};

export const familyOf = (runtime: string): Family => RUNTIMES.find((r) => r.id === runtime)?.family ?? "python";

/** A function's settings, as its detail page gives them. */
export type Fn = {
  name: string;
  arn: string;
  description: string;
  runtime: string;
  handler: string;
  memory: number;
  timeout: number;
  ephemeral: number;
  role: string;
  roleName: string;
  state: string;
  pkg: string;
  arch: string;
  layers: string[];
  env: Record<string, string>;
  reserved: number | null;
  url: string | null;
  urlAuth: string | null;
  cors: UrlCors | null;
  lastModified: string;
};

export type UrlCors = { AllowOrigins?: string[]; AllowMethods?: string[]; AllowHeaders?: string[]; ExposeHeaders?: string[]; MaxAge?: number; AllowCredentials?: boolean };

const text = (d: ResourceDetail, label: string): string | null => d.fields.find((f) => f.label === label)?.value ?? null;
const json = <T>(v: string | null): T | null => {
  if (!v) return null;
  try {
    return JSON.parse(v) as T;
  } catch {
    return null;
  }
};

/** The last part of an ARN (`arn:aws:iam::1:role/path/lambda-exec` is `lambda-exec`). */
export const roleArn = (account: string, name: string): string => `arn:aws:iam::${account}:role/${name}`;

export const tailOf = (arn: string): string => arn.split(/[/:]/).filter(Boolean).pop() ?? arn;

export function fnOf(d: ResourceDetail): Fn {
  const url = text(d, "Function URL");
  const role = text(d, "Execution role") ?? "";
  const reserved = text(d, "Reserved concurrency");
  return {
    name: d.name,
    arn: text(d, "ARN") ?? "",
    description: text(d, "Description") ?? "",
    runtime: text(d, "Runtime") ?? "",
    handler: text(d, "Handler") ?? "",
    memory: Number(text(d, "Memory (MB)") ?? 128),
    timeout: Number(text(d, "Timeout") ?? 3),
    ephemeral: Number(text(d, "Ephemeral storage (MB)") ?? 512),
    role,
    roleName: tailOf(role),
    state: text(d, "State") ?? "",
    pkg: text(d, "Package type") ?? "Zip",
    arch: text(d, "Architecture") ?? "x86_64",
    layers: (json<{ Arn: string }[]>(text(d, "Layers")) ?? []).map((l) => l.Arn),
    env: json<Record<string, string>>(text(d, "Environment variables")) ?? {},
    reserved: reserved === null ? null : Number(reserved),
    url: url && url !== "None" ? url : null,
    urlAuth: text(d, "Function URL auth"),
    cors: json<UrlCors>(text(d, "Function URL CORS")),
    lastModified: text(d, "Last modified") ?? "",
  };
}

/** Something that invokes a function without the function being asked: its words (`SNS topic alerts`) and the console page of it. */
export type Trigger = { how: "mapping" | "policy"; kind: string; name: string; words: string; to: Wired["to"] | null; id?: string; enabled?: boolean; batch?: number; source?: string };

/** What an event source mapping reads from, by its ARN: `SQS queue email-outbox`. */
export function sourceWords(arn: string): { kind: string; words: string; to: Wired["to"] | null } {
  const name = tailOf(arn);
  const service = arn.split(":")[2] ?? "";
  if (service === "sqs") return { kind: "SQS queue", words: `SQS queue ${name}`, to: { service: "sqs", kind: "queues", name } };
  if (service === "kinesis") return { kind: "Kinesis stream", words: `Kinesis stream ${name}`, to: { service: "kinesis", kind: "streams", name } };
  if (service === "dynamodb") return { kind: "DynamoDB stream", words: `DynamoDB stream of ${arn.split("/")[1] ?? name}`, to: { service: "dynamodb", kind: "tables", name: arn.split("/")[1] ?? name } };
  return { kind: "Event source", words: `${service || "source"} ${name}`, to: null };
}

type Statement = { Sid?: string; Effect?: string; Principal?: unknown; Action?: string | string[]; Condition?: Record<string, Record<string, string>> };

/** The statements of a function's resource-based policy (GetPolicy's `Policy` text); none when there is none. */
export function statementsOf(policy: string | undefined | null): Statement[] {
  const doc = json<{ Statement?: Statement | Statement[] }>(policy ?? null);
  if (!doc?.Statement) return [];
  return Array.isArray(doc.Statement) ? doc.Statement : [doc.Statement];
}

/** The principal of a statement as text (`sns.amazonaws.com`, an account's ARN, `*`). */
export function principalOf(s: Statement): string {
  const p = s.Principal;
  if (typeof p === "string") return p;
  if (p && typeof p === "object") {
    const v = Object.values(p as Record<string, string | string[]>).flat();
    return v.join(", ");
  }
  return "";
}

/** The condition of a statement as one line (`ArnLike aws:SourceArn arn:…`), `–` when it has none. */
export function conditionOf(s: Statement): string {
  const out: string[] = [];
  for (const [op, kv] of Object.entries(s.Condition ?? {})) for (const [k, v] of Object.entries(kv)) out.push(`${op} ${k.replace(/^AWS:/, "aws:")} ${v}`);
  return out.join("; ");
}

/** The triggers a statement names, when its principal is a service that invokes on events and its condition names the resource. */
export function policyTrigger(s: Statement): Trigger | null {
  const principal = principalOf(s);
  const source = Object.values(s.Condition ?? {}).flatMap((kv) => Object.entries(kv)).find(([k]) => /sourcearn$/i.test(k))?.[1];
  const name = source ? tailOf(source.replace(/^arn:aws:s3:::/, "s3/")) : "";
  if (principal === "sns.amazonaws.com" && source) return { how: "policy", kind: "SNS topic", name, words: `SNS topic ${name}`, to: { service: "sns", kind: "topics", name }, id: s.Sid };
  if (principal === "s3.amazonaws.com" && source) return { how: "policy", kind: "S3 bucket", name: source.replace("arn:aws:s3:::", ""), words: `S3 bucket ${source.replace("arn:aws:s3:::", "")}`, to: { service: "s3", kind: "buckets", name: source.replace("arn:aws:s3:::", "") }, id: s.Sid };
  if (principal === "events.amazonaws.com" && source) {
    const rule = source.split(":rule/")[1] ?? name;
    return { how: "policy", kind: "EventBridge rule", name: rule, words: `EventBridge rule ${rule.split("/").pop()}`, to: { service: "eventbridge", kind: "rules", name: rule.includes("/") ? rule : `default/${rule}` }, id: s.Sid };
  }
  if (principal === "apigateway.amazonaws.com") return { how: "policy", kind: "API Gateway", name: "API Gateway", words: "API Gateway", to: null, id: s.Sid };
  return null;
}

/** Every trigger of a function: its event source mappings (the rows of the Event source mappings list), then the services its
 * policy lets invoke it. */
export function triggersOf(mappings: { name: string; cells: (string | null)[] }[], statements: Statement[]): Trigger[] {
  const fromMappings = mappings.map((m): Trigger => {
    const arn = m.cells[4] ?? m.cells[0] ?? "";
    const s = sourceWords(arn);
    return { how: "mapping", kind: s.kind, name: tailOf(arn), words: s.words, to: s.to, id: m.cells[3] ?? m.name, enabled: m.cells[1] !== "Disabled", batch: Number(m.cells[2] ?? 0), source: arn };
  });
  return [...statements.flatMap((s) => policyTrigger(s) ?? []), ...fromMappings];
}

/** `a`, `a and b`, `a, b and c`. */
export function list(items: readonly string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** The sentence under the function's name: what it runs, what triggers it, and when it last ran. */
export function sentenceOf(f: Fn, triggers: readonly Trigger[], lastRan: string | null): string {
  const parts = [`${f.name} runs ${runtimeName(f.runtime) ? `${runtimeName(f.runtime)} ` : ""}with ${f.memory} MB and a ${f.timeout} second timeout.`];
  const who = triggers.map((t) => t.words);
  if (who.length > 0) parts.push(`${list(who)} ${who.length === 1 ? "triggers" : "trigger"} it${lastRan ? `; it last ran at ${lastRan}.` : "."}`);
  else parts.push(lastRan ? `Nothing triggers it; it last ran at ${lastRan}.` : "Nothing triggers it yet.");
  return parts.join(" ");
}

/** The sentence of a function that was just made. */
export const madeSentence = (name: string, roleName: string, triggers: number): string => `${name} was created a moment ago with the Hello world example. It has ${triggers === 0 ? "no trigger yet" : `${triggers} trigger${triggers === 1 ? "" : "s"}`}, and runs as ${roleName}.`;

/** The strip under the sentence: what triggers it, then (after the function) the role it runs as. */
export function wiredOf(f: Fn, triggers: readonly Trigger[]): Wired[] {
  const out: Wired[] = triggers.filter((t) => t.to).map((t) => ({ label: "Triggered by", to: t.to as Wired["to"], note: null }));
  // A function nothing triggers has no strip: the role it runs as is on its Configuration tab.
  if (out.length > 0 && f.role) out.push({ label: "Runs as", to: { service: "iam", kind: "roles", name: f.roleName }, note: null });
  return out;
}

/** A chip's words in the strip (`SNS topic alerts`, `role lambda-exec`). */
export function chipWords(w: Pick<Wired, "label" | "to">, triggers: readonly Trigger[]): string {
  if (w.label === "Runs as") return `role ${w.to.name}`;
  return triggers.find((t) => t.to?.service === w.to.service && t.to.name === w.to.name)?.words ?? w.to.name;
}
