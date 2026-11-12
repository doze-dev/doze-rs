// The allow-list: crates/kafka-console/design/allow.json, an array of
//   { "board": "home", "field": "button", "selector": "Connect an app [pri] -> to:connect", "reason": "...", "expires": "9a", "render": "fixture" }
// `field` is a signature field (or "*"), `selector` (optional) names the one difference: it must equal the difference's
// value, "scope: value", or "kind value". An entry without a selector silences every difference of that field on that
// board. `reason` is required. `expires` (optional) is the slice tag by which the entry must be gone. `render`
// (optional) limits the entry to the "fixture" or the "live" render.
//
// Errors (the gate fails on them): not an array, an unknown board, an unknown field, no reason, a bad `expires`, and a
// stale entry (one that silences nothing on a board that was compared).

import { type Difference, FIELDS } from "./compare.ts";

export interface AllowEntry {
  board: string;
  field: string;
  selector?: string;
  reason: string;
  expires?: string;
  render?: "fixture" | "live";
}

export const SLICES = ["9a", "9b", "9c", "9d", "9e", "9f", "9g", "9h", "9i", "close"];

export function validateAllow(raw: unknown, boardIds: Set<string>, currentSlice?: string): { entries: AllowEntry[]; errors: string[] } {
  const errors: string[] = [];
  if (!Array.isArray(raw)) return { entries: [], errors: ["allow.json must be a JSON array"] };
  const entries: AllowEntry[] = [];
  raw.forEach((e, i) => {
    const at = `allow.json[${i}]`;
    if (!e || typeof e !== "object") return errors.push(`${at}: not an object`);
    const x = e as Record<string, unknown>;
    if (typeof x.board !== "string" || !boardIds.has(x.board)) return errors.push(`${at}: unknown board ${JSON.stringify(x.board)}`);
    if (typeof x.field !== "string" || !(x.field === "*" || FIELDS.includes(x.field))) errors.push(`${at} (${x.board}): unknown field ${JSON.stringify(x.field)} (one of ${FIELDS.join(", ")} or *)`);
    if (typeof x.reason !== "string" || !x.reason.trim()) errors.push(`${at} (${x.board}): an entry needs a reason`);
    if (x.selector !== undefined && typeof x.selector !== "string") errors.push(`${at} (${x.board}): selector must be a string`);
    if (x.render !== undefined && x.render !== "fixture" && x.render !== "live") errors.push(`${at} (${x.board}): render must be "fixture" or "live"`);
    if (x.expires !== undefined) {
      if (typeof x.expires !== "string" || !SLICES.includes(x.expires)) errors.push(`${at} (${x.board}): expires must be one of ${SLICES.join(", ")}`);
      else if (currentSlice && SLICES.indexOf(x.expires) < SLICES.indexOf(currentSlice)) errors.push(`${at} (${x.board}): expired with slice ${x.expires} (now ${currentSlice}): fix the page or renew the entry with a reason`);
    }
    entries.push(x as unknown as AllowEntry);
  });
  return { entries, errors };
}

export function matches(e: AllowEntry, d: Difference, render: "fixture" | "live"): boolean {
  if (e.render && e.render !== render) return false;
  if (e.field !== "*" && e.field !== d.field) return false;
  if (e.selector === undefined) return true;
  return e.selector === d.value || e.selector === `${d.scope}: ${d.value}` || e.selector === `${d.kind} ${d.value}`;
}
