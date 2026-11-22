// The Visual policy editor's model: a policy as permission blocks (an effect, one service and its actions, and the resources), and back.
// A policy the blocks cannot say (a Condition, a NotAction, a Principal) is not shown as blocks: the editor says why and the JSON stays the
// way to write it. Going to the blocks and back may restructure the document (a statement over two services becomes two blocks), never
// what it allows. Pure.

export type Effect = "Allow" | "Deny";
export type Block = {
  sid: string | null;
  effect: Effect;
  /** The IAM prefix (`sqs`), `*` for every service, or `""` while none is chosen. */
  service: string;
  /** All of the service's actions (`sqs:*`, or `*` for every service). */
  all: boolean;
  /** Action names without the prefix (`SendMessage`, `Get*`); used when `all` is off. */
  actions: string[];
  /** Every resource (`*`). */
  anyResource: boolean;
  resources: string[];
};

export const newBlock = (service = ""): Block => ({ sid: null, effect: "Allow", service, all: false, actions: [], anyResource: false, resources: [""] });

const asList = (v: unknown): string[] | null => (typeof v === "string" ? [v] : Array.isArray(v) && v.every((x) => typeof x === "string") ? (v as string[]) : null);

/** The blocks of a policy document, or why it cannot be shown as blocks. */
export function blocksOf(text: string): { blocks: Block[] } | { why: string } {
  let doc: unknown;
  try {
    doc = JSON.parse(text);
  } catch {
    return { why: "The JSON is not valid yet." };
  }
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) return { why: "A policy is a JSON object." };
  const d = doc as Record<string, unknown>;
  const raw = Array.isArray(d.Statement) ? d.Statement : d.Statement === undefined ? [] : [d.Statement];
  const blocks: Block[] = [];
  for (let i = 0; i < raw.length; i++) {
    const s = raw[i] as Record<string, unknown> | null;
    const at = `Statement ${i + 1}`;
    if (typeof s !== "object" || s === null || Array.isArray(s)) return { why: `${at} is not an object.` };
    for (const key of ["Condition", "NotAction", "NotResource", "Principal", "NotPrincipal"]) if (s[key] !== undefined) return { why: `${at} has ${key}, which the blocks do not show. Edit it as JSON.` };
    if (s.Effect !== "Allow" && s.Effect !== "Deny") return { why: `${at} has no Effect of Allow or Deny.` };
    const actions = asList(s.Action ?? []);
    const resources = s.Resource === undefined ? [] : asList(s.Resource);
    if (actions === null || resources === null) return { why: `${at} has an Action or Resource that is not text.` };
    const sid = typeof s.Sid === "string" ? s.Sid : null;
    const anyResource = resources.length === 1 && resources[0] === "*";
    const rest = { sid, effect: s.Effect as Effect, anyResource, resources: anyResource ? [] : resources };
    if (actions.includes("*")) {
      blocks.push({ ...rest, service: "*", all: true, actions: [] });
      continue;
    }
    const by = new Map<string, string[]>();
    for (const a of actions) {
      const [prefix, ...name] = a.split(":");
      if (!prefix || name.length === 0) return { why: `${at} names the action ${JSON.stringify(a)}, which is not service:Action.` };
      by.set(prefix, [...(by.get(prefix) ?? []), name.join(":")]);
    }
    if (by.size === 0) blocks.push({ ...rest, service: "", all: false, actions: [] });
    for (const [service, names] of by) blocks.push({ ...rest, service, all: names.includes("*"), actions: names.filter((n) => n !== "*") });
  }
  return { blocks };
}

/** The action names a block says, with their prefix: `["sqs:SendMessage"]`, `["sqs:*"]` or `["*"]`; empty when none is chosen. */
export function actionsOf(b: Block): string[] {
  if (b.service === "*") return b.all ? ["*"] : [];
  if (b.service === "") return [];
  return b.all ? [`${b.service}:*`] : b.actions.map((a) => `${b.service}:${a}`);
}

/** The policy document of the blocks. A block with no action, or no resource, leaves that element out: the editor's own check says so. */
export function documentOf(blocks: readonly Block[]): string {
  const statements = blocks.map((b) => {
    const actions = actionsOf(b);
    const resources = b.anyResource ? ["*"] : b.resources.map((r) => r.trim()).filter((r) => r !== "");
    return { ...(b.sid ? { Sid: b.sid } : {}), Effect: b.effect, ...(actions.length > 0 ? { Action: actions } : {}), ...(resources.length > 0 ? { Resource: b.anyResource ? "*" : resources.length === 1 ? resources[0] : resources } : {}) };
  });
  return JSON.stringify({ Version: "2012-10-17", Statement: statements }, null, 2);
}

/** What is not finished in a block, one sentence, or `null`. */
export function blockProblem(b: Block): string | null {
  if (b.service === "") return "Choose a service.";
  if (!b.all && b.actions.length === 0) return "Choose at least one action, or all of the service's.";
  if (!b.anyResource && b.resources.every((r) => r.trim() === "")) return "Add the ARN of a resource, or choose all resources.";
  const bad = b.anyResource ? undefined : b.resources.map((r) => r.trim()).find((r) => r !== "" && r !== "*" && !r.startsWith("arn:"));
  return bad === undefined ? null : `${bad} is not an ARN: a resource is arn:partition:service:region:account:resource.`;
}
