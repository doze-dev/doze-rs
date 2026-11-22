// What a rule's targets fan out to past the rule: an SNS topic target has subscribers, and the strip on the rule's page says how many and of what
// (`3 subscribed queues`), as the board draws it (eventbridge__rule-detail).
import { useQueries } from "@tanstack/react-query";
import { RESOURCE_PATHS } from "../../../api/resources";
import { client } from "../../../api/client";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { count, targetParts } from "./words";
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
