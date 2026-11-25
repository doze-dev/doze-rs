// Inputs saved in this browser under a name: at most thirty for each region. Pure: `kept.ts` keeps the list, `saved-list.tsx` draws it.
import type { Ask } from "./history";

export const MOST_SAVED = 30;
export const MOST_NAME = 80;

export type Saved = Ask & { id: string; name: string; at: number };

/** The list with `item` in it: first, replacing the one of the same name (without regard to case); or the words of why it is not kept. */
export function saveInput(list: Saved[], item: Saved): { ok: true; list: Saved[] } | { ok: false; why: string } {
  const name = item.name.trim();
  if (name === "") return { ok: false, why: "Give it a name." };
  if (name.length > MOST_NAME) return { ok: false, why: `A name is at most ${MOST_NAME} characters.` };
  // eslint-disable-next-line no-control-regex -- a name with a control character is refused
  if (/[\u0000-\u001f\u007f]/.test(name)) return { ok: false, why: "A name has no control characters." };
  const same = (s: Saved) => s.name.toLowerCase() === name.toLowerCase();
  const rest = list.filter((s) => !same(s));
  if (rest.length >= MOST_SAVED) return { ok: false, why: `${MOST_SAVED} inputs are kept. Delete one first.` };
  return { ok: true, list: [{ ...item, name }, ...rest] };
}

const text = (v: unknown, most = 200): v is string => typeof v === "string" && v.length > 0 && v.length <= most;

/** One entry as it was kept, or `null` when it is not whole. */
export function parseSaved(v: unknown): Saved | null {
  if (v === null || typeof v !== "object") return null;
  const o = v as Record<string, unknown>;
  if (!text(o.id) || !text(o.name, MOST_NAME) || typeof o.at !== "number" || !text(o.target) || !text(o.service) || !text(o.title) || !text(o.op)) return null;
  if (o.input === null || typeof o.input !== "object" || Array.isArray(o.input)) return null;
  return { id: o.id, name: o.name, at: o.at, target: o.target, service: o.service, title: o.title, op: o.op, input: o.input as Record<string, unknown> };
}
