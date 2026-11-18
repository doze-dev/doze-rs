// The Live page's address: the filters (the same names the server reads), the mode, and the overlays. Each parser keeps a value
// only when it is one the page can use, so a hand-edited address is a page with no such filter, never an error.
import { shortText } from "../../lib/search";

export type LiveSearch = {
  q?: string;
  client?: string[];
  api?: string[];
  topic?: string[];
  /** Refused only. */
  refused?: true;
  /** Only the requests a Lab rule touched: its number, or `all`. */
  lab?: number | "all";
  /** Show the polls that found nothing (hidden by default). */
  idle?: true;
  /** The page back: rows older than this id (snapshot only). */
  before?: number;
  /** The Live tail is on. */
  tail?: true;
  /** The request Sheet. */
  request?: number;
  /** The clients Sheet. */
  clients?: true;
};

const MOST_VALUES = 32;

function list(raw: unknown): string[] | undefined {
  const all = Array.isArray(raw) ? raw : typeof raw === "string" ? [raw] : [];
  const out: string[] = [];
  for (const v of all) {
    const t = typeof v === "string" ? v.trim().slice(0, 128) : typeof v === "number" ? String(v) : "";
    if (t && !out.includes(t) && out.length < MOST_VALUES) out.push(t);
  }
  return out.length ? out : undefined;
}

function flag(raw: unknown): true | undefined {
  return raw === true || raw === 1 || raw === "1" || raw === "true" ? true : undefined;
}

function id(raw: unknown): number | undefined {
  const n = typeof raw === "number" ? raw : typeof raw === "string" && /^\d{1,15}$/.test(raw) ? Number(raw) : NaN;
  return Number.isSafeInteger(n) && n >= 0 ? n : undefined;
}

function labOf(raw: unknown): number | "all" | undefined {
  if (raw === "all") return "all";
  const t = typeof raw === "number" ? String(raw) : raw;
  return typeof t === "string" && /^[1-9]\d{0,8}$/.test(t) ? Number(t) : undefined;
}

export function liveSearch(s: Record<string, unknown>): LiveSearch {
  return {
    q: shortText(typeof s.q === "string" ? s.q.trim() : s.q, 128),
    client: list(s.client),
    api: list(s.api),
    topic: list(s.topic),
    refused: flag(s.refused),
    lab: labOf(s.lab),
    idle: flag(s.idle),
    before: id(s.before),
    tail: flag(s.tail),
    request: id(s.request),
    clients: flag(s.clients),
  };
}

/** The filters as the server reads them (`?q=..&client=a&client=b&refused=1&idle=0`), without the mode and the overlays. */
export function apiQuery(s: LiveSearch, withBefore = true): string {
  const p = new URLSearchParams();
  if (s.q) p.append("q", s.q);
  for (const v of s.client ?? []) p.append("client", v);
  for (const v of s.api ?? []) p.append("api", v);
  for (const v of s.topic ?? []) p.append("topic", v);
  if (s.refused) p.append("refused", "1");
  if (s.lab) p.append("lab", String(s.lab));
  if (s.idle) p.append("idle", "0");
  if (withBefore && s.before !== undefined) p.append("before", String(s.before));
  return p.toString();
}

/** Whether any filter is on (what an empty tape is empty because of). */
export function filtered(s: LiveSearch): boolean {
  return !!(s.q || s.client?.length || s.api?.length || s.topic?.length || s.refused || s.lab);
}
