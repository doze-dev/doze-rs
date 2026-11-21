// Whether a message satisfies a subscription's filter policy, for the publish page's preview. It reads the rules of SNS's filter
// policy that a message's attributes decide by plain comparison (exact strings and numbers, prefix, suffix, anything-but, exists,
// equals-ignore-case, numeric comparisons); a policy that uses more says `unknown` and the page does not guess. doze-aws's own
// matcher (crates/aws-sns/src/filter_match.rs) is what decides a delivery: this is what the page tells beforehand.
export type Attribute = { type: "String" | "String.Array" | "Number" | "Binary"; value: string };
export type Verdict = "match" | "no-match" | "unknown";

const values = (a: Attribute | undefined): (string | number)[] | null => {
  if (!a || a.type === "Binary") return null;
  if (a.type === "Number") return Number.isFinite(Number(a.value)) ? [Number(a.value)] : null;
  if (a.type === "String.Array") {
    try {
      const v: unknown = JSON.parse(a.value);
      return Array.isArray(v) ? v.filter((x): x is string | number => typeof x === "string" || typeof x === "number") : null;
    } catch {
      return null;
    }
  }
  return [a.value];
};

/** One condition against the values of the attribute: `true`, `false`, or `null` when it is a rule this does not read. */
function condition(c: unknown, have: (string | number)[] | null): boolean | null {
  if (typeof c === "string") return have?.some((v) => v === c) ?? false;
  if (typeof c === "number") return have?.some((v) => v === c) ?? false;
  if (typeof c !== "object" || c === null || Array.isArray(c)) return null;
  const rule = c as Record<string, unknown>;
  const keys = Object.keys(rule);
  if (keys.length !== 1) return null;
  const key = keys[0]!;
  const arg = rule[key];
  const strings = (have ?? []).filter((v): v is string => typeof v === "string");
  switch (key) {
    case "exists":
      return typeof arg === "boolean" ? (have !== null) === arg : null;
    case "prefix":
      return typeof arg === "string" ? strings.some((v) => v.startsWith(arg)) : null;
    case "suffix":
      return typeof arg === "string" ? strings.some((v) => v.endsWith(arg)) : null;
    case "equals-ignore-case":
      return typeof arg === "string" ? strings.some((v) => v.toLowerCase() === arg.toLowerCase()) : null;
    case "anything-but": {
      const not = Array.isArray(arg) ? arg : [arg];
      if (!not.every((n) => typeof n === "string" || typeof n === "number")) return null;
      return have !== null && have.length > 0 && have.every((v) => !not.includes(v));
    }
    case "numeric": {
      if (!Array.isArray(arg) || arg.length % 2 !== 0) return null;
      const nums = (have ?? []).filter((v): v is number => typeof v === "number");
      return nums.some((n) => {
        for (let i = 0; i < arg.length; i += 2) {
          const op = arg[i];
          const b = arg[i + 1];
          if (typeof b !== "number") return false;
          const ok = op === "=" ? n === b : op === ">" ? n > b : op === ">=" ? n >= b : op === "<" ? n < b : op === "<=" ? n <= b : null;
          if (ok === null) return false;
          if (!ok) return false;
        }
        return true;
      });
    }
    default:
      return null;
  }
}

/** The verdict of a policy (JSON text, attribute scope) on the message's attributes. */
export function matches(policy: string, attributes: Record<string, Attribute>): Verdict {
  let doc: unknown;
  try {
    doc = JSON.parse(policy);
  } catch {
    return "unknown";
  }
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) return "unknown";
  let unknown = false;
  for (const [key, want] of Object.entries(doc as Record<string, unknown>)) {
    if (key === "$or" || !Array.isArray(want)) return "unknown";
    const have = values(attributes[key]);
    let any = false;
    for (const c of want) {
      const r = condition(c, have);
      if (r === null) unknown = true;
      else if (r) any = true;
    }
    if (!any) return unknown ? "unknown" : "no-match";
  }
  return unknown ? "unknown" : "match";
}

/** `event in [order_placed, order_cancelled]`: a policy of plain string values, said short; the JSON text when it is more. */
export function policyWords(policy: string): string {
  try {
    const doc: unknown = JSON.parse(policy);
    if (typeof doc !== "object" || doc === null || Array.isArray(doc)) return policy;
    const parts = Object.entries(doc as Record<string, unknown>).map(([k, v]) => (Array.isArray(v) && v.every((x) => typeof x === "string" || typeof x === "number") ? `${k} in [${v.join(", ")}]` : null));
    return parts.every((p) => p !== null) ? parts.join(" and ") : policy;
  } catch {
    return policy;
  }
}
