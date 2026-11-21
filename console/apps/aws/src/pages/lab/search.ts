// The Lab's address (which API and operation are open) and how its input is read: small, so the router does not pull the
// page in.

export type LabSearch = { api?: string; op?: string };

export function labSearch(s: Record<string, unknown>): LabSearch {
  const word = (v: unknown) => (typeof v === "string" && /^[A-Za-z0-9-]{1,64}$/.test(v) ? v : undefined);
  return { api: word(s.api), op: word(s.op) };
}

/** Whether `text` is a JSON object; the parse error's words when it is not. */
export function readInput(text: string): { ok: true; value: Record<string, unknown> } | { ok: false; why: string } {
  try {
    const v: unknown = JSON.parse(text);
    if (v === null || typeof v !== "object" || Array.isArray(v)) return { ok: false, why: "The input is a JSON object: { … }." };
    return { ok: true, value: v as Record<string, unknown> };
  } catch (e) {
    return { ok: false, why: e instanceof Error ? e.message : "Not JSON." };
  }
}
