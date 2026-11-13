// The Query hooks of the Topics pages (slice R1a). Every page is a snapshot (live-design.md): staleTime Infinity, the
// RefreshControl's interval the only timer, keepPreviousData so nothing blanks while a refresh or a filter is in flight.
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { intervalMs, useRefreshInterval } from "@/lib/refresh";
import { api } from "./client";
import type { CompactDone } from "./types/CompactDone";
import type { ConsumersResponse } from "./types/ConsumersResponse";
import type { LogResponse } from "./types/LogResponse";
import type { MessagesResponse } from "./types/MessagesResponse";
import type { ProducersResponse } from "./types/ProducersResponse";
import type { RecordView } from "./types/RecordView";
import type { TopicFrame } from "./types/TopicFrame";
import type { TopicsResponse } from "./types/TopicsResponse";
import type { TxnResponse } from "./types/TxnResponse";

const seg = encodeURIComponent;

export const topicKeys = {
  all: ["topics"] as const,
  list: (q: string, internal: boolean) => ["topics", "list", q, internal] as const,
  topic: (t: string) => ["topics", "one", t] as const,
  frame: (t: string) => ["topics", "one", t, "frame"] as const,
  messages: (t: string, qs: string) => ["topics", "one", t, "messages", qs] as const,
  record: (t: string, p: number, o: number) => ["topics", "one", t, "record", p, o] as const,
  consumers: (t: string) => ["topics", "one", t, "consumers"] as const,
  log: (t: string, p: number) => ["topics", "one", t, "log", p] as const,
  producers: (t: string, p: number) => ["topics", "one", t, "producers", p] as const,
  txn: (t: string, id: string) => ["topics", "one", t, "txn", id] as const,
};

function useInterval(): number | false {
  const [v] = useRefreshInterval();
  const ms = intervalMs(v);
  return ms > 0 ? ms : false;
}

/** A snapshot query that follows the RefreshControl. */
function snapshot<T>(key: readonly unknown[], path: string, refetchInterval: number | false, enabled = true) {
  return {
    queryKey: key,
    queryFn: ({ signal }: { signal: AbortSignal }) => api<T>(path, {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
    enabled,
    retry: false,
  };
}

export function useTopics(q: string, internal: boolean) {
  const i = useInterval();
  const qs = new URLSearchParams();
  if (q) qs.set("q", q);
  if (internal) qs.set("internal", "1");
  const s = qs.toString();
  return useQuery(snapshot<TopicsResponse>(topicKeys.list(q, internal), `/api/v1/topics${s ? `?${s}` : ""}`, i));
}

export function useTopicFrame(topic: string) {
  const i = useInterval();
  return useQuery({ ...snapshot<TopicFrame>(topicKeys.frame(topic), `/api/v1/topics/${seg(topic)}`, i), placeholderData: undefined });
}

/** `qs` is the API's query (`partition=0&isolation=read_committed`), built by `messagesQuery`. */
export function useMessages(topic: string, qs: string) {
  const i = useInterval();
  return useQuery({
    ...snapshot<MessagesResponse>(topicKeys.messages(topic, qs), `/api/v1/topics/${seg(topic)}/messages${qs ? `?${qs}` : ""}`, i),
    // Keep the rows on screen while a filter changes, but never another topic's.
    placeholderData: (prev: MessagesResponse | undefined) => (prev && prev.view.head.name === topic ? prev : undefined),
  });
}

export function useRecord(topic: string, at: { partition: number; offset: number } | null) {
  return useQuery({
    queryKey: topicKeys.record(topic, at?.partition ?? -1, at?.offset ?? -1),
    queryFn: ({ signal }) => api<RecordView>(`/api/v1/topics/${seg(topic)}/records/${at!.partition}/${at!.offset}`, {}, signal),
    staleTime: Infinity,
    enabled: !!at,
    retry: false,
  });
}

export function useConsumers(topic: string) {
  const i = useInterval();
  return useQuery(snapshot<ConsumersResponse>(topicKeys.consumers(topic), `/api/v1/topics/${seg(topic)}/consumers`, i));
}

export function useLog(topic: string, partition: number) {
  const i = useInterval();
  return useQuery({
    ...snapshot<LogResponse>(topicKeys.log(topic, partition), `/api/v1/topics/${seg(topic)}/log?partition=${partition}`, i),
    placeholderData: (prev: LogResponse | undefined) => (prev && prev.view.topic === topic ? prev : undefined),
  });
}

export function useProducers(topic: string, partition: number) {
  const i = useInterval();
  return useQuery({
    ...snapshot<ProducersResponse>(topicKeys.producers(topic, partition), `/api/v1/topics/${seg(topic)}/producers?partition=${partition}`, i),
    placeholderData: (prev: ProducersResponse | undefined) => (prev && prev.view.topic === topic ? prev : undefined),
  });
}

export function useTxn(topic: string, id: string) {
  const i = useInterval();
  return useQuery({ ...snapshot<TxnResponse>(topicKeys.txn(topic, id), `/api/v1/topics/${seg(topic)}/transactions/${seg(id)}`, i), placeholderData: undefined });
}

/** The Lab's Compact now: the cleaner's real round on one partition. Refreshes the topic's pages after. */
export function useCompactNow(topic: string, partition: number) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api<CompactDone>(`/api/v1/topics/${seg(topic)}/log/compact?partition=${partition}`, { method: "POST" }),
    onSettled: () => void qc.invalidateQueries({ queryKey: topicKeys.topic(topic) }),
  });
}
