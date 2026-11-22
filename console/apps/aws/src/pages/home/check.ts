// The connection check: the newest call an app made to this address, counted only from `since` (0: since doze-aws started).
// "Check again" moves `since` to now, so the check starts over and only a call made after it ticks the row.
import type { CallRecord } from "../../api/types/CallRecord";
import type { HomeView } from "../../api/types/HomeView";

/** The newest call from an app that was answered after `since`; null when none was. */
export function heardSince(home: Pick<HomeView, "recent">, since: number): CallRecord | null {
  return home.recent.find((c) => c.at_ms > since) ?? null;
}

/** The AWS CLI call that makes a call of its own to the address, with no resource to exist first. */
export function probe(endpoint: string): string {
  return `aws sts get-caller-identity \\\n  --endpoint-url ${endpoint}`;
}
