// Workflow Studio's Design mode, as pure functions over a definition: the states a person can drag onto the canvas, where one may be
// dropped (on an edge of the graph), and what dropping, renaming and deleting do to the definition. Each returns a new definition (the old
// one is not touched) or `null` for a change that cannot be made (a state that ends a path, put in the middle of one). The definition they
// make may still be invalid (a choice with no rule): that is for ValidateStateMachineDefinition to say, which the studio asks.
import type { AslState, Definition } from "./graph";

/** A state the browser offers. `kind` says how it joins a path: `linear` (it has a Next or ends), `choice`, or `terminal` (Succeed, Fail). */
export type Offer = { id: string; label: string; group: "flow" | "action" | "pattern"; kind: "linear" | "choice" | "terminal"; service?: string; make: () => AslState; /** More states it brings (a failure to catch to); `refer` points the state at the name the extra one was given. */ extra?: { name: string; state: AslState; refer: (s: AslState, name: string) => void }; hint?: string; /** The name its state is first given, when it is not the label. */ name?: string };

const pass = (): AslState => ({ Type: "Pass", End: true });

const branch = (name: string): Definition => ({ StartAt: name, States: { [name]: pass() } });

const task = (resource: string, parameters: Record<string, unknown>): AslState => ({ Type: "Task", Resource: resource, Parameters: parameters, End: true });

export const FLOW: Offer[] = [
  { id: "choice", label: "Choice", group: "flow", kind: "choice", make: () => ({ Type: "Choice", Choices: [{ Variable: "$.value", StringEquals: "", Next: "" }], Default: "" }) },
  { id: "parallel", label: "Parallel", group: "flow", kind: "linear", make: () => ({ Type: "Parallel", Branches: [{ StartAt: "Pass", States: { Pass: pass() } }], End: true }) },
  { id: "map", label: "Map", group: "flow", kind: "linear", make: () => ({ Type: "Map", ItemsPath: "$.items", ItemProcessor: { StartAt: "Pass", States: { Pass: pass() } }, End: true }) },
  { id: "pass", label: "Pass", group: "flow", kind: "linear", make: pass },
  { id: "wait", label: "Wait", group: "flow", kind: "linear", make: () => ({ Type: "Wait", Seconds: 5, End: true }) },
  { id: "succeed", label: "Success", group: "flow", kind: "terminal", make: () => ({ Type: "Succeed" }) },
  { id: "fail", label: "Fail", group: "flow", kind: "terminal", make: () => ({ Type: "Fail", Error: "Error", Cause: "" }) },
];

/** What doze-aws can run as a Task: the integrations its Step Functions serves (crates/aws-sfn/src/integ). */
export const ACTIONS: Offer[] = [
  { id: "lambda-invoke", label: "Lambda Invoke", group: "action", kind: "linear", service: "Lambda", make: () => task("arn:aws:states:::lambda:invoke", { FunctionName: "my-function", "Payload.$": "$" }) },
  { id: "sqs-send", label: "SQS SendMessage", group: "action", kind: "linear", service: "SQS", make: () => task("arn:aws:states:::sqs:sendMessage", { QueueUrl: "http://127.0.0.1:4566/000000000000/my-queue", "MessageBody.$": "States.JsonToString($)" }) },
  { id: "sns-publish", label: "SNS Publish", group: "action", kind: "linear", service: "SNS", make: () => task("arn:aws:states:::sns:publish", { TopicArn: "arn:aws:sns:us-east-1:000000000000:my-topic", "Message.$": "States.JsonToString($)" }) },
  { id: "events-put", label: "EventBridge PutEvents", group: "action", kind: "linear", service: "EventBridge", make: () => task("arn:aws:states:::events:putEvents", { Entries: [{ Source: "my.app", DetailType: "event", "Detail.$": "States.JsonToString($)" }] }) },
  { id: "ddb-get", label: "DynamoDB GetItem", group: "action", kind: "linear", service: "DynamoDB", make: () => task("arn:aws:states:::dynamodb:getItem", { TableName: "my-table", Key: { id: { "S.$": "$.id" } } }) },
  { id: "ddb-put", label: "DynamoDB PutItem", group: "action", kind: "linear", service: "DynamoDB", make: () => task("arn:aws:states:::dynamodb:putItem", { TableName: "my-table", Item: { id: { "S.$": "$.id" } } }) },
  { id: "ddb-update", label: "DynamoDB UpdateItem", group: "action", kind: "linear", service: "DynamoDB", make: () => task("arn:aws:states:::dynamodb:updateItem", { TableName: "my-table", Key: { id: { "S.$": "$.id" } }, UpdateExpression: "SET #s = :s", ExpressionAttributeNames: { "#s": "status" }, ExpressionAttributeValues: { ":s": { S: "done" } } }) },
  { id: "ddb-delete", label: "DynamoDB DeleteItem", group: "action", kind: "linear", service: "DynamoDB", make: () => task("arn:aws:states:::dynamodb:deleteItem", { TableName: "my-table", Key: { id: { "S.$": "$.id" } } }) },
  { id: "s3-put", label: "S3 PutObject", group: "action", kind: "linear", service: "S3", make: () => task("arn:aws:states:::aws-sdk:s3:putObject", { Bucket: "my-bucket", Key: "key", "Body.$": "States.JsonToString($)" }) },
  { id: "sfn-start", label: "Step Functions StartExecution", group: "action", kind: "linear", service: "Step Functions", make: () => task("arn:aws:states:::states:startExecution", { StateMachineArn: "arn:aws:states:us-east-1:000000000000:stateMachine:other", "Input.$": "$" }) },
];

/** Pieces of several states that go together. */
export const PATTERNS: Offer[] = [
  {
    id: "retry-catch",
    label: "Lambda Invoke with Retry and Catch",
    name: "Lambda Invoke",
    group: "pattern",
    kind: "linear",
    service: "Lambda",
    hint: "Retries any error three times, then goes to a Fail state.",
    make: () => ({ ...task("arn:aws:states:::lambda:invoke", { FunctionName: "my-function", "Payload.$": "$" }), Retry: [{ ErrorEquals: ["States.ALL"], IntervalSeconds: 2, MaxAttempts: 3, BackoffRate: 2 }], Catch: [{ ErrorEquals: ["States.ALL"], Next: "" }] }),
    extra: { name: "Handle failure", state: { Type: "Fail", Error: "TaskFailed", Cause: "The task failed after its retries" }, refer: (s, name) => { if (s.Catch?.[0]) s.Catch[0].Next = name; } },
  },
  {
    id: "fan-out",
    label: "Parallel with two branches",
    name: "Parallel",
    group: "pattern",
    kind: "linear",
    hint: "Two branches that run at once; the next state gets both results.",
    make: () => ({ Type: "Parallel", Branches: [branch("Pass"), branch("Pass (1)")], End: true }),
  },
];

export const OFFERS: Offer[] = [...FLOW, ...ACTIONS, ...PATTERNS];
export const offerOf = (id: string): Offer | undefined => OFFERS.find((o) => o.id === id);

/** The path of a nested graph: `Par/0/` is the first branch of the state `Par` (a Map's item processor is `/0/` too). */
export type Scope = { state: string; index: number }[];

export function parseScope(scope: string): Scope {
  const parts = scope.split("/").filter((p) => p !== "");
  const out: Scope = [];
  for (let i = 0; i + 1 < parts.length; i += 2) out.push({ state: parts[i] ?? "", index: Number(parts[i + 1]) });
  return out;
}

/** An edge of the graph, as the definition has it: which state it leaves (`null`: Start), which it enters (`null`: End), by which field. */
export type EdgeRef = { scope: string; from: string | null; to: string | null; kind: "start" | "next" | "choice" | "default" | "end"; nth: number };

/** The definition (or branch, or item processor) `scope` names inside `root`. */
function containerOf(root: Definition, scope: Scope): Definition | null {
  let at: Definition | undefined = root;
  for (const s of scope) {
    const st: AslState | undefined = at?.States?.[s.state];
    at = st?.Branches?.[s.index] ?? st?.ItemProcessor ?? st?.Iterator;
    if (!at) return null;
  }
  return at ?? null;
}

/** Every state name in a definition, in branches too. */
export function namesOf(def: Definition): string[] {
  const out: string[] = [];
  const walk = (d: Definition | undefined) => {
    for (const [n, s] of Object.entries(d?.States ?? {})) {
      out.push(n);
      for (const b of s.Branches ?? []) walk(b);
      walk(s.ItemProcessor);
      walk(s.Iterator);
    }
  };
  walk(def);
  return out;
}

/** `Pass`, then `Pass (1)`, `Pass (2)`: a name no state of the definition has. */
export function uniqueName(def: Definition, base: string): string {
  const taken = new Set(namesOf(def));
  if (!taken.has(base)) return base;
  for (let i = 1; ; i++) if (!taken.has(`${base} (${i})`)) return `${base} (${i})`;
}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

/** The name a state of this offer is first given: `Pass`, `Lambda Invoke`, `Choice`. */
export const nameFor = (o: Offer): string => o.name ?? o.label;

/** Drops `offer` on `edge`: the new state takes the place between the two states the edge joins. */
export function insertOnEdge(def: Definition, edge: EdgeRef, offer: Offer): { def: Definition; name: string } | null {
  const next = clone(def);
  const c = containerOf(next, parseScope(edge.scope));
  if (!c) return null;
  c.States ??= {};
  const name = uniqueName(next, nameFor(offer));
  const state = offer.make();
  const to = edge.to;
  const join = (s: AslState) => {
    delete s.End;
    delete s.Next;
    if (to === null) s.End = true;
    else s.Next = to;
  };
  if (offer.kind === "terminal" && to !== null) return null;
  if (offer.kind === "linear") join(state);
  if (offer.extra) {
    const more = uniqueName({ States: { ...c.States, [name]: {} } }, offer.extra.name);
    c.States[more] = offer.extra.state;
    offer.extra.refer(state, more);
  }
  if (offer.kind === "terminal") {
    delete state.End;
    delete state.Next;
  }
  if (offer.kind === "choice") {
    const rule = uniqueName({ States: { ...c.States, [name]: {} } }, "Pass");
    const other = uniqueName({ States: { ...c.States, [name]: {}, [rule]: {} } }, "Pass");
    c.States[name] = state;
    c.States[rule] = pass();
    state.Choices = [{ Variable: "$.value", StringEquals: "", Next: rule }];
    if (to === null) {
      c.States[other] = pass();
      state.Default = other;
    } else state.Default = to;
  } else c.States[name] = state;
  if (edge.kind === "start") c.StartAt = name;
  else {
    const from = edge.from === null ? undefined : c.States[edge.from];
    if (!from) return null;
    if (edge.kind === "next" || edge.kind === "end") {
      delete from.End;
      from.Next = name;
    } else if (edge.kind === "default") from.Default = name;
    else if (edge.kind === "choice") {
      const rule = from.Choices?.[edge.nth];
      if (!rule) return null;
      rule.Next = name;
    }
  }
  return { def: next, name };
}

/** The state `name`, and the `States` object it is in. */
function find(def: Definition, name: string): { states: Record<string, AslState>; owner: Definition } | null {
  const walk = (d: Definition | undefined): { states: Record<string, AslState>; owner: Definition } | null => {
    if (!d?.States) return null;
    if (d.States[name]) return { states: d.States, owner: d };
    for (const s of Object.values(d.States)) {
      for (const b of s.Branches ?? []) {
        const f = walk(b);
        if (f) return f;
      }
      const f = walk(s.ItemProcessor) ?? walk(s.Iterator);
      if (f) return f;
    }
    return null;
  };
  return walk(def);
}

export function stateOf(def: Definition, name: string): AslState | undefined {
  return find(def, name)?.states[name];
}

/** Every reference in `d` (one scope) to `from` becomes a reference to `to`. */
function retarget(d: Definition, from: string, to: string | null): void {
  if (d.StartAt === from && to !== null) d.StartAt = to;
  for (const s of Object.values(d.States ?? {})) {
    if (s.Next === from) {
      if (to === null) {
        delete s.Next;
        s.End = true;
      } else s.Next = to;
    }
    if (s.Default === from && to !== null) s.Default = to;
    for (const r of s.Choices ?? []) if (r.Next === from && to !== null) r.Next = to;
    for (const c of s.Catch ?? []) if (c.Next === from && to !== null) c.Next = to;
  }
}

/** Takes state `name` out; whatever led to it leads on to what it led to. */
export function deleteState(def: Definition, name: string): Definition | null {
  const next = clone(def);
  const f = find(next, name);
  if (!f) return null;
  const s = f.states[name] as AslState;
  const onward = s.Next ?? null;
  delete f.states[name];
  retarget(f.owner, name, onward);
  if (f.owner.StartAt === name) {
    const first = Object.keys(f.states)[0];
    f.owner.StartAt = onward ?? first ?? "";
  }
  return next;
}

/** Renames a state, and everything that names it. A name that is taken (or empty) is refused. */
export function renameState(def: Definition, from: string, to: string): Definition | null {
  if (to === from) return def;
  if (to.trim() === "" || namesOf(def).includes(to)) return null;
  const next = clone(def);
  const f = find(next, from);
  if (!f) return null;
  const rebuilt: Record<string, AslState> = {};
  for (const [n, s] of Object.entries(f.states)) rebuilt[n === from ? to : n] = s;
  f.owner.States = rebuilt;
  retarget(f.owner, from, to);
  return next;
}

/** Replaces a state's body. */
export function setState(def: Definition, name: string, state: AslState): Definition | null {
  const next = clone(def);
  const f = find(next, name);
  if (!f) return null;
  f.states[name] = state;
  return next;
}
