// The members of a query-protocol request come flat (`Attributes.entry.1.key`, `Tags.member.1.Key`): the call as a person means it is
// nested (`Attributes: {DisplayName: "x"}`, `Tags: [{Key, Value}]`). Pure.

type Node = { [k: string]: Node } | string;

function build(flat: Record<string, unknown>): Record<string, Node> {
  const root: Record<string, Node> = {};
  for (const [key, value] of Object.entries(flat)) {
    const parts = key.split(".");
    let at = root;
    parts.forEach((p, i) => {
      if (i === parts.length - 1) at[p] = typeof value === "string" ? value : JSON.stringify(value);
      else {
        const next = at[p];
        at[p] = typeof next === "object" ? next : {};
        at = at[p] as Record<string, Node>;
      }
    });
  }
  return root;
}

const numbered = (n: { [k: string]: Node }): boolean => Object.keys(n).length > 0 && Object.keys(n).every((k) => /^\d+$/.test(k));

function finish(n: Node): unknown {
  if (typeof n === "string") return n;
  const keys = Object.keys(n);
  // `member` wraps a list and `entry` a map's entries; `Tags.member.1.Key` is the first element of `Tags`.
  const only = keys.length === 1 ? keys[0]! : null;
  if (only === "member" || only === "entry") {
    const inner = n[only]!;
    if (typeof inner !== "string" && numbered(inner)) {
      const items = Object.keys(inner).sort((a, b) => Number(a) - Number(b)).map((k) => finish(inner[k]!));
      if (only === "entry" && items.every((i) => typeof i === "object" && i !== null && "key" in i && "value" in i)) return Object.fromEntries((items as { key: string; value: unknown }[]).map((i) => [i.key, i.value]));
      return items;
    }
  }
  if (numbered(n)) return Object.keys(n).sort((a, b) => Number(a) - Number(b)).map((k) => finish(n[k]!));
  return Object.fromEntries(keys.map((k) => [k, finish(n[k]!)]));
}

/** `flat` as the nested document its dotted keys describe; as it is when no key is dotted. */
export function unflatten(flat: Record<string, unknown>): Record<string, unknown> {
  if (!Object.keys(flat).some((k) => k.includes("."))) return flat;
  return finish(build(flat)) as Record<string, unknown>;
}
