// What a page does with its reads when something is deleted: the gone resource's own reads are NOT made again for a while (a read
// of what is gone is refused, and the browser logs the refusal as an error), and the rest of the service's reads are made afresh.
// One helper for every delete flow of every service.
//
// A read that is merely marked stale is made again by the next page that shows it, so the gone reads are kept as they were for a
// minute instead. And a list nobody shows is dropped, not kept stale: the page that opens next would draw its old rows, with the
// gone one among them, and read each row at once, before the list is read again. The answer is a promise that settles when the
// lists on screen have been read again: a mutation that returns it opens the next page (the list) on rows that are current.
import { type QueryClient, type QueryKey, useQueryClient } from "@tanstack/react-query";
import { forgetGone } from "../shell/pins";

/** Whether `value` (a part of a query key: a string, a list, the input object of an operation) holds `name`: as a string of its
 * own, or as a part of an address or an ARN (`/restapis/<id>/stages`, `arn:aws:sqs:r:a:<name>`). */
export function mentions(value: unknown, name: string): boolean {
  if (typeof value === "string") return value === name || (name !== "" && value.split(/[/:]/).includes(name));
  if (Array.isArray(value)) return value.some((v) => mentions(v, name));
  if (value !== null && typeof value === "object") return Object.values(value).some((v) => mentions(v, name));
  return false;
}

/** The keys of a service's reads that do not carry its name as a segment (CloudFormation's own are `["cfn", ...]`). */
const OWN_KEYS: Readonly<Record<string, readonly string[]>> = { cloudformation: ["cfn", "cfn-preview"] };

/** What a service's reads are looked for by: its name and the keys of its own. */
export const scopeOf = (service: string): readonly string[] => [service, ...(OWN_KEYS[service] ?? [])];

/** The service's reads are keyed with its name in them (`["resources", "one", "sqs", "queues", name]`, `["cfn", ...]`):
 * `scope` is what to look for, the service's name and any key of its own. */
const ofService = (key: QueryKey, scope: readonly string[]) => scope.some((s) => key.some((k) => k === s));

/** How long the reads of what was deleted are kept as they were (the page of it, and the lists that still show it, leave in less). */
export const GONE_FOR_MS = 60_000;

/**
 * After the delete of `names` (a resource, or the rules of a bulk delete) in a service: the reads that name any of them are not
 * made again for `GONE_FOR_MS` (the page is on screen until the list opens, and the list is on screen until it is read again),
 * the lists and the other reads of the service are read again, and the index of Resources is counted again when next shown.
 * Settles once the reads on screen have been made again.
 */
export function forgetDeleted(qc: QueryClient, scope: readonly string[], names: readonly string[], now: () => number = Date.now): Promise<void> {
  const gone = (key: QueryKey) => names.some((n) => mentions(key, n));
  // What was deleted is not somewhere to go back to: the rail's Pinned and Recent let it go.
  if (scope[0]) forgetGone(scope[0], names);
  const until = now() + GONE_FOR_MS;
  for (const q of qc.getQueryCache().findAll({ predicate: (q) => ofService(q.queryKey, scope) && gone(q.queryKey) })) {
    qc.setQueryDefaults(q.queryKey, { staleTime: () => (now() < until ? Number.POSITIVE_INFINITY : 0) });
  }
  const rest = qc.getQueryCache().findAll({ predicate: (q) => ofService(q.queryKey, scope) && !gone(q.queryKey) });
  for (const q of rest) if (q.getObserversCount() === 0) qc.getQueryCache().remove(q);
  void qc.invalidateQueries({ queryKey: ["resources", "index"] });
  return qc.invalidateQueries({ predicate: (q) => ofService(q.queryKey, scope) && !gone(q.queryKey) });
}

/** `forgetDeleted`, for a component: call the result with the service's scope and the names that were deleted. */
export function useForgetDeleted(): (scope: readonly string[], names: readonly string[]) => Promise<void> {
  const qc = useQueryClient();
  return (scope, names) => forgetDeleted(qc, scope, names);
}
