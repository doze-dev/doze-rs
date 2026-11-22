// What a rule's targets fan out to past the rule: an SNS topic target has subscribers, and the strip on the rule's page says how many and of what
// (`3 subscribed queues`), as the board draws it (eventbridge__rule-detail).
import { useQueries, useQuery } from "@tanstack/react-query";
import { RESOURCE_PATHS } from "../../../api/resources";
import { client } from "../../../api/client";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { arnName, count, list, targetParts } from "./words";
import type { TargetItem } from "./types";

/** `3 subscribed queues`, `1 subscribed function`, `2 subscribers` (of several kinds); null when there are none. `protocols`: one per subscription. */
export function fanoutWords(protocols: readonly string[]): string | null {
  if (protocols.length === 0) return null;
  const kinds = new Set(protocols);
  const only = kinds.size === 1 ? [...kinds][0] : undefined;
  if (only === "sqs") return count(protocols.length, "subscribed queue");
  if (only === "lambda") return count(protocols.length, "subscribed function");
  return count(protocols.length, "subscriber");
}

/** For each SNS topic the targets send to, what its subscribers are, said as a chip (the topics that have none are left out). */
export function useFanout(targets: readonly TargetItem[] | undefined): string[] {
  const topics = [...new Set((targets ?? []).map((t) => targetParts(t.Arn)).filter((p) => p.kind === "SNS topic").map((p) => p.name))];
  const reads = useQueries({
    queries: topics.map((name) => ({ queryKey: ["resources", "one", "sns", "topics", name], queryFn: ({ signal }: { signal: AbortSignal }) => client.api<ResourceDetail>(RESOURCE_PATHS.one("sns", "topics", name), {}, signal), retry: false })),
  });
  return reads.flatMap((r) => {
    const rows = r.data?.subs.find((s) => s.title === "Subscriptions")?.rows ?? [];
    const words = fanoutWords(rows.map((row) => row.cells[1] ?? ""));
    return words ? [words] : [];
  });
}

/** `orders-created already fans out to invoice-queue, inventory-queue and analytics-queue.` A queue or function is named, not its ARN; `null` when the topic has no subscribers. `subs`: one per subscription. */
export function fanoutHint(topic: string, subs: readonly { protocol: string; endpoint: string }[]): string | null {
  if (subs.length === 0) return null;
  const names = subs.map((s) => (s.protocol === "sqs" || s.protocol === "lambda" ? arnName(s.endpoint) : s.endpoint));
  const shown = names.length > 5 ? [...names.slice(0, 5), `${names.length - 5} more`] : names;
  return `${topic} already fans out to ${list(shown)}.`;
}

/** What the SNS topic called `name` already sends to, said as `fanoutHint` says it; `null` while unread, unknown or unsubscribed. */
export function useTopicHint(name: string | null): string | null {
  const read = useQuery({ queryKey: ["resources", "one", "sns", "topics", name ?? ""], queryFn: ({ signal }) => client.api<ResourceDetail>(RESOURCE_PATHS.one("sns", "topics", name ?? ""), {}, signal), enabled: name !== null, retry: false });
  const rows = read.data?.subs.find((s) => s.title === "Subscriptions")?.rows ?? [];
  return name ? fanoutHint(name, rows.map((row) => ({ protocol: row.cells[1] ?? "", endpoint: row.cells[2] ?? "" }))) : null;
}
