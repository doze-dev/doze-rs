// What the pages of KMS, Secrets Manager and Parameter Store read: a whole list (every page, up to a bound), and the lists of the
// other services that tell what a key encrypts.
import { useEffect, useMemo } from "react";
import { useAbout } from "../../api/queries";
import { usePrincipals } from "../../api/spaces";
import { useResourceList } from "../../api/resources";
import type { CallRecord } from "../../api/types/CallRecord";
import type { ColumnSpec } from "../../api/types/ColumnSpec";
import type { ResourceList } from "../../api/types/ResourceList";
import type { ResourceRow } from "../../api/types/ResourceRow";
import { callerWords, namedKeys } from "../../kit/calls";
import { isConsole } from "../live/search";
import { cell } from "./cfg-parts";

/** The most pages a page reads on its own: a list longer than this says so. */
const PAGES = 10;

/** Every row of a kind's list (reading the next pages as they come), the columns, the first page (for its titles) and the state of the read. */
export function useAllRows(service: string, kind: string, { all = false, params = {}, enabled = true }: { all?: boolean; params?: Record<string, string>; enabled?: boolean } = {}) {
  const q = useResourceList(service, kind, all, params);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = q;
  const pages = q.data?.pages.length ?? 0;
  useEffect(() => {
    if (enabled && hasNextPage && !isFetchingNextPage && pages < PAGES) void fetchNextPage();
  }, [enabled, hasNextPage, isFetchingNextPage, fetchNextPage, pages]);
  const first: ResourceList | undefined = q.data?.pages[0];
  const rows: ResourceRow[] = useMemo(() => q.data?.pages.flatMap((p) => p.rows) ?? [], [q.data]);
  const columns: ColumnSpec[] = first?.columns ?? [];
  return { q, first, rows, columns, loading: q.isPending, more: Boolean(hasNextPage) };
}

export type KeyUse = { service: "sqs" | "secretsmanager" | "ssm"; kind: "queues" | "secrets" | "parameters"; name: string; noun: string; key: string | null };

/** What is encrypted with a key of this account: the queues' KMS key, the secrets' encryption key and the SecureString parameters'
 * key, as the other services' own lists give them. Only the services this doze-aws serves are read. */
export function useKeyUses(enabled = true): { uses: KeyUse[]; loading: boolean } {
  const about = useAbout().data;
  const has = (s: string) => about?.services.some((x) => x.name === s && x.served) ?? false;
  const queues = useAllRows("sqs", "queues", { enabled: enabled && has("sqs") });
  const secrets = useAllRows("secretsmanager", "secrets", { enabled: enabled && has("secretsmanager") });
  const params = useAllRows("ssm", "parameters", { enabled: enabled && has("ssm") });
  const uses = useMemo<KeyUse[]>(
    () => [
      ...queues.rows.map((r) => ({ service: "sqs" as const, kind: "queues" as const, name: r.name, noun: "SQS queue", key: cell(queues.columns, r, "KMS key") })),
      ...secrets.rows.map((r) => ({ service: "secretsmanager" as const, kind: "secrets" as const, name: r.name, noun: "secret", key: cell(secrets.columns, r, "Encryption key") })),
      ...params.rows.map((r) => ({ service: "ssm" as const, kind: "parameters" as const, name: r.name, noun: "parameter", key: cell(params.columns, r, "KMS key") })),
    ],
    [queues.rows, queues.columns, secrets.rows, secrets.columns, params.rows, params.columns],
  );
  return { uses, loading: (enabled && has("sqs") && queues.loading) || (enabled && has("secretsmanager") && secrets.loading) || (enabled && has("ssm") && params.loading) };
}

/** Who read a resource lately, from its Recent calls: the calls to the operations that read it (`GetSecretValue`), not the console's own. */
export function useReaders(calls: readonly CallRecord[], reads: readonly string[]): { who: string[]; count: number; last: number | null } {
  const about = useAbout().data;
  const principals = usePrincipals(namedKeys(calls));
  const titles = new Map((about?.services ?? []).map((s) => [s.name, s.title]));
  const names = new Map((principals.data?.principals ?? []).map((p) => [p.key, p]));
  const read = calls.filter((c) => c.action !== null && reads.includes(c.action) && !isConsole(c) && c.status < 400);
  const who = [...new Set(read.map((c) => callerWords(c, titles, names).label))];
  return { who, count: read.length, last: read[0]?.at_ms ?? null };
}
