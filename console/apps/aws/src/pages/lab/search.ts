// The Lab's address (which tab, which API and operation are open) and how its input is read: small, so the router does not pull
// the page in.

export const TABS = ["rehearse", "send"] as const;
export type LabTab = (typeof TABS)[number];

export type LabSearch = { tab?: LabTab; api?: string; op?: string };

export function labSearch(s: Record<string, unknown>): LabSearch {
  const word = (v: unknown) => (typeof v === "string" && /^[A-Za-z0-9-]{1,64}$/.test(v) ? v : undefined);
  const tab = TABS.find((t) => t === s.tab);
  return { tab, api: word(s.api), op: word(s.op) };
}

/** The tab an address shows: the one it names, else Send a call for an address that names an operation (the Lab's old address), else
 * Rehearse. */
export function tabOf(s: LabSearch): LabTab {
  return s.tab ?? (s.api || s.op ? "send" : "rehearse");
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
