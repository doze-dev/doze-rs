// Reading a topic's subscriptions one by one (the list says where each delivers; the settings of each are one read of its own).
import { useQueries } from "@tanstack/react-query";
import { client } from "../../../api/client";
import { RESOURCE_PATHS } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { type Subscription, subscriptionOf } from "./sub-form-model";
import type { SubRow } from "./sub-model";

/** The most subscriptions the page reads the settings of for a sentence. */
export const MOST = 6;

/** Each confirmed subscription's settings (raw delivery, filter policy), read when there are few enough to say something of. */
export function useSubscriptions(rows: readonly SubRow[], enabled = true) {
  const confirmed = rows.filter((r) => !r.pending);
  const on = enabled && confirmed.length > 0 && confirmed.length <= MOST;
  const reads = useQueries({
    queries: confirmed.slice(0, MOST).map((r) => ({
      queryKey: ["resources", "one", "sns", "subscriptions", r.name],
      queryFn: ({ signal }: { signal: AbortSignal }) => client.api<ResourceDetail>(RESOURCE_PATHS.one("sns", "subscriptions", r.name), {}, signal),
      retry: false,
      enabled: on,
    })),
  });
  const ready = on && reads.every((q) => q.data);
  const subs: Subscription[] = ready ? reads.map((q) => subscriptionOf(q.data!)) : [];
  return { ready, subs };
}
