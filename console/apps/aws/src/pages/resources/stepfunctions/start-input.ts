// What Start execution opens with. The input of the machine's last run that succeeded is what a person starts it with again (the board's
// "the input of the machine's last run"); a machine that has not succeeded yet gets a skeleton of the paths its first state reads, each
// filled with something that is not empty, because a DynamoDB key, an SQS message body and an SNS message are refused when they are `""`.
// Pure, so it is tested without a browser.
import type { Definition } from "./graph";

/** What a path is read as: the type the state it sits in wants of it. */
type Kind = "string" | "number" | "numeric-string" | "boolean";

/** The type a key wants of the path it holds: `"N.$"` is a DynamoDB number (a numeric string), `NumericLessThanPath` names a number. */
const kindOf = (key: string): Kind => (key === "N.$" ? "numeric-string" : key === "BOOL.$" || /^Boolean\w+Path$/.test(key) ? "boolean" : /^Numeric\w+Path$/.test(key) ? "number" : "string");

/** A Choice rule's operator and the value it compares the variable to (`StringEquals: "paid"` gives `paid`), when the value is one to use. */
function compared(rule: Record<string, unknown>): { kind: Kind; value: unknown } | null {
  for (const [op, v] of Object.entries(rule)) {
    const m = /^(String|Numeric|Boolean)(Equals|GreaterThan|LessThan|GreaterThanEquals|LessThanEquals)(Path)?$/.exec(op);
    if (!m) continue;
    // The value is a path to another member (`NumericLessThanPath`): the variable has the type, and the path is scanned for its own.
    if (m[3]) return { kind: m[1] === "Numeric" ? "number" : m[1] === "Boolean" ? "boolean" : "string", value: undefined };
    if (m[1] === "String" && typeof v === "string") return { kind: "string", value: v };
    if (m[1] === "Boolean" && typeof v === "boolean") return { kind: "boolean", value: v };
    if (m[1] === "Numeric" && typeof v === "number") return { kind: "number", value: m[2]?.startsWith("Greater") && m[2] !== "GreaterThanEquals" ? v + 1 : m[2] === "LessThan" ? v - 1 : v };
  }
  return null;
}

function placeholder(name: string, kind: Kind, value?: unknown): unknown {
  if (value !== undefined) return value;
  if (kind === "number") return 1;
  if (kind === "numeric-string") return "1";
  if (kind === "boolean") return true;
  return name;
}

/** A skeleton of the input a definition's first state reads (`$.order_id` in its Parameters or its InputPath, a Choice's Variable), or
 * `{}` when it reads none. A state's ResultPath, OutputPath and ResultSelector write, they do not read, and `$$.` is the context
 * object: neither is the input's. */
export function inputSkeleton(def: Definition | null | undefined): string {
  const start = def?.States?.[def.StartAt ?? ""];
  if (!start) return "{}";
  const found = new Map<string, { kind: Kind; value?: unknown }>();
  const note = (text: string, kind: Kind, value?: unknown) => {
    for (const m of text.matchAll(/(?<![$\w])\$\.([A-Za-z_][\w-]*(?:\.[A-Za-z_][\w-]*)*)/g)) {
      const path = m[1] as string;
      const had = found.get(path);
      if (!had || (had.value === undefined && value !== undefined)) found.set(path, { kind, value });
    }
  };
  const scan = (v: unknown, kind: Kind = "string"): void => {
    if (typeof v === "string") note(v, kind);
    else if (Array.isArray(v)) v.forEach((x) => scan(x, kind));
    else if (v !== null && typeof v === "object") {
      const rule = v as Record<string, unknown>;
      const cmp = typeof rule.Variable === "string" ? compared(rule) : null;
      if (cmp) note(rule.Variable as string, cmp.kind, cmp.value);
      for (const [k, x] of Object.entries(rule)) {
        if (k === "ResultPath" || k === "OutputPath" || k === "ResultSelector" || (cmp && k === "Variable")) continue;
        scan(x, kindOf(k));
      }
    }
  };
  scan(start);
  if (found.size === 0) return "{}";
  const input: Record<string, unknown> = {};
  for (const [path, { kind, value }] of [...found].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) {
    const keys = path.split(".");
    let at = input;
    keys.forEach((k, i) => {
      if (i === keys.length - 1) {
        if (!(k in at)) at[k] = placeholder(k, kind, value);
      } else {
        const next = at[k];
        if (typeof next === "object" && next !== null) at = next as Record<string, unknown>;
        else if (!(k in at) || typeof next === "string") {
          const made: Record<string, unknown> = {};
          at[k] = made;
          at = made;
        }
      }
    });
  }
  return JSON.stringify(input, null, 2);
}
