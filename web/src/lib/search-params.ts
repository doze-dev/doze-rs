// The address's search params, as a person reads them: a list is the same name repeated (`?topic=orders&topic=clickstream`),
// not JSON (`?topic=%5B%22orders%22%5D`, TanStack's default). Given to the router as `parseSearch` and `stringifySearch`.
//
// Old links keep working: a value written as JSON by an older build (`["orders"]`, `"12"`, `true`) is read as it was meant.
// A text that would read as another type (`true`, `12`, or one starting with `[`, `{` or `"`) is written quoted, as before.

const AMBIGUOUS = /^(?:true|false|null|-?\d+(?:\.\d+)?(?:e[+-]?\d+)?)$|^[[{"]/i;

function readValue(v: string): unknown {
  if (/^[[{"]/.test(v)) {
    try {
      return JSON.parse(v);
    } catch {
      return v;
    }
  }
  if (v === "true") return true;
  if (v === "false") return false;
  if (/^-?\d{1,15}$/.test(v)) return Number(v);
  return v;
}

function writeValue(v: unknown): string {
  if (typeof v === "string") return AMBIGUOUS.test(v) ? JSON.stringify(v) : v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return JSON.stringify(v);
}

export function parseSearch(raw: string): Record<string, unknown> {
  const q = new URLSearchParams(raw.startsWith("?") ? raw.slice(1) : raw);
  const out: Record<string, unknown> = {};
  for (const key of new Set(q.keys())) {
    const all = q.getAll(key).map(readValue);
    out[key] = all.length > 1 ? all : all[0];
  }
  return out;
}

export function stringifySearch(search: Record<string, unknown>): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(search)) {
    if (v === undefined || v === null) continue;
    if (Array.isArray(v)) for (const x of v) q.append(k, writeValue(x));
    else q.set(k, writeValue(v));
  }
  const s = q.toString();
  return s ? `?${s}` : "";
}
