// What Home's "What is awake" says: each service of the build in its group, when anything last called it, and whether that
// keeps it awake. A service lets go of its memory and files after `--idle-secs` without a request (300 by default), so one whose
// newest call is older than that is asleep. Pure: the page draws what this returns.
import type { ServiceRow } from "../../api/types/ServiceRow";
import type { ServiceSeen } from "../../api/types/ServiceSeen";

export type Awake = "awake" | "asleep" | "off";

export type AwakeRow = {
  name: string;
  title: string;
  /** The newest call that reached it (an app's, this console's, another service's), or null when none is in the record. */
  lastMs: number | null;
  calls: number;
  state: Awake;
};

export type AwakeGroup = { name: string; rows: AwakeRow[] };

export const STATE_WORDS: Record<Awake, string> = { awake: "Awake", asleep: "Asleep", off: "Not served" };

/** The services by group, in the catalogue's order. `served: false` is a service this run left out (`--services`): it is never
 * awake, and it says so. */
export function awakeGroups(services: readonly ServiceRow[], activity: readonly ServiceSeen[], idleSecs: number, nowMs: number): AwakeGroup[] {
  const seen = new Map(activity.map((a) => [a.service, a]));
  const groups: AwakeGroup[] = [];
  for (const s of services) {
    const a = seen.get(s.name);
    const lastMs = a?.last_ms ?? null;
    const state: Awake = !s.served ? "off" : lastMs !== null && nowMs - lastMs <= idleSecs * 1000 ? "awake" : "asleep";
    const row: AwakeRow = { name: s.name, title: s.title, lastMs, calls: a?.calls ?? 0, state };
    const g = groups.find((x) => x.name === s.group);
    if (g) g.rows.push(row);
    else groups.push({ name: s.group, rows: [row] });
  }
  return groups;
}

/** `1 of 3 awake`. */
export function awakeCount(groups: readonly AwakeGroup[]): { awake: number; served: number; all: number } {
  const rows = groups.flatMap((g) => g.rows);
  return { awake: rows.filter((r) => r.state === "awake").length, served: rows.filter((r) => r.state !== "off").length, all: rows.length };
}
