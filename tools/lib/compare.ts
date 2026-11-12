// Compares a board's signature with a rendered page's signature, field by field, and says what is missing, extra or out of order.
// Pure functions; board-diff.ts reads the files and prints.

import { type Mode, type Signature, align, alignTables, factsOf } from "../board-signature.ts";

export interface Difference {
  /** `missing` = the board has it and the page does not; `extra` = the page has it and the board does not; `order` = same members, different order. */
  kind: "missing" | "extra" | "order";
  /** The signature field (see Fact.field in board-signature.ts). */
  field: string;
  /** Where (`table#0`, `card:This instance`, `tabs`, or empty for the page). */
  scope: string;
  value: string;
}

export const FIELDS = [
  "kind", "title", "h1", "heading", "table", "table.col", "table.rows", "tab", "tab.on", "button", "link", "card", "dt", "field", "note", "extra", "op", "state", "landmark",
];

const key = (f: { field: string; scope: string; value: string }): string => `${f.field}\u0000${f.scope}\u0000${f.value}`;

export function compare(board: Signature, rendered: Signature, mode: Mode): Difference[] {
  const aligned = align(board, rendered);
  const fb = factsOf(board, mode);
  const fr = factsOf(aligned, mode, alignTables(board, rendered));
  const out: Difference[] = [];

  const count = (facts: Array<{ field: string; scope: string; value: string }>) => {
    const m = new Map<string, { n: number; f: (typeof facts)[number] }>();
    for (const f of facts) {
      const e = m.get(key(f));
      if (e) e.n++;
      else m.set(key(f), { n: 1, f });
    }
    return m;
  };
  const cb = count(fb.facts);
  const cr = count(fr.facts);
  for (const [k, { n, f }] of cb) {
    const m = (cr.get(k)?.n ?? 0);
    for (let i = m; i < n; i++) out.push({ kind: "missing", field: f.field, scope: f.scope, value: f.value });
  }
  for (const [k, { n, f }] of cr) {
    const m = (cb.get(k)?.n ?? 0);
    for (let i = m; i < n; i++) out.push({ kind: "extra", field: f.field, scope: f.scope, value: f.value });
  }

  // Order: only where the members are the same.
  const reported = new Set(out.map((d) => `${d.field}\u0000${d.scope}`));
  for (const sb of fb.sequences) {
    const sr = fr.sequences.find((s) => s.field === sb.field && s.scope === sb.scope);
    if (!sr || reported.has(`${sb.field}\u0000${sb.scope}`)) continue;
    if (sb.values.length === sr.values.length && sb.values.join("\u0001") !== sr.values.join("\u0001")) {
      out.push({ kind: "order", field: sb.field, scope: sb.scope, value: `${sr.values.join(" | ")}  (board: ${sb.values.join(" | ")})` });
    }
  }
  return out;
}

export const describe = (d: Difference): string => `${d.kind.padEnd(7)} ${d.field}${d.scope ? ` [${d.scope}]` : ""}: ${d.value}`;
