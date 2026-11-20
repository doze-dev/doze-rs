// The deep pages' queries (crates/aws-console/src/api/deep.rs): one operation of a service, its request written as the
// service's model has it, made through the bus as the caller `console` (Live shows it). Nothing here polls: a read is made when
// its page opens and when the person refreshes it; a Live Tail is the service's own stream, open while the page asks for it.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import { client } from "./client";
import type { Invoked } from "./types/Invoked";

const enc = encodeURIComponent;

export const DEEP_PATHS = {
  call: (service: string, operation: string) => `/api/v1/deep/${enc(service)}/${enc(operation)}`,
  tail: (group: string, streams: string[], pattern: string) => {
    const q = [`group=${enc(group)}`, ...streams.map((s) => `stream=${enc(s)}`), pattern.trim() ? `pattern=${enc(pattern)}` : ""].filter(Boolean).join("&");
    return `/api/v1/deep/logs/tail?${q}`;
  },
};

const post = (body: unknown): RequestInit => ({ method: "POST", body: JSON.stringify(body), headers: { "Content-Type": "application/json" } });

/** Calls `operation` of `service` with `body` once (the page's own request). */
export function deepCall<T>(service: string, operation: string, body: unknown, signal?: AbortSignal): Promise<T> {
  return client.api<T>(DEEP_PATHS.call(service, operation), post(body), signal);
}

/** A read, made when the page opens (and again on Refresh). `body` is part of the key: a new request is a new read. */
export function useDeepRead<T>(service: string, operation: string, body: unknown, enabled = true) {
  return useQuery({
    queryKey: ["deep", service, operation, body],
    queryFn: ({ signal }) => deepCall<T>(service, operation, body, signal),
    enabled,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/** A change. Done, the reads of the same service are stale and read again. */
export function useDeepWrite<T>(service: string, operation: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: unknown) => deepCall<T>(service, operation, body),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["deep", service] });
      void qc.invalidateQueries({ queryKey: ["resources", "one", service] });
    },
  });
}

/** `Invoke`, synchronous: the payload as text, the answer with the log tail decoded. */
export function useInvoke() {
  return useMutation({ mutationFn: (req: { FunctionName: string; Payload: string; LogType: "Tail" | "None"; Qualifier?: string }) => deepCall<Invoked>("lambda", "Invoke", req) });
}

/** One event of a Live Tail session, as CloudWatch Logs sends it (`sessionResults`). */
export type TailEvent = { logStreamName?: string; logGroupIdentifier?: string; message?: string; timestamp?: number; ingestionTime?: number };

/** The most events a Live Tail page keeps; the oldest go past it. */
export const TAIL_KEPT = 1000;

/** A Live Tail session: open while `on`, its events newest first. The updates are the service's (one a second while open). */
export function useLiveTail(group: string, streams: string[], pattern: string) {
  const [on, setOn] = useState(false);
  const [events, setEvents] = useState<TailEvent[]>([]);
  const [state, setState] = useState<{ status: "off" | "opening" | "open" | "ended"; error?: { code: string; message: string } }>({ status: "off" });
  const [sampled, setSampled] = useState(false);
  const key = `${group}|${streams.join(",")}|${pattern}`;
  const asked = useRef({ group, streams, pattern });
  useEffect(() => {
    asked.current = { group, streams, pattern };
  });
  useEffect(() => {
    if (!on) return;
    const ac = new AbortController();
    client
      .streamEvents(
        DEEP_PATHS.tail(asked.current.group, asked.current.streams, asked.current.pattern),
        (e) => {
          // An event read after the session was let go (Stop, or the page left) is not shown.
          if (ac.signal.aborted) return;
          if (e.event === "start") setState({ status: "open" });
          else if (e.event === "update") {
            const update = JSON.parse(e.data) as { sessionResults?: TailEvent[]; sessionMetadata?: { sampled?: boolean } };
            if (update.sessionMetadata?.sampled) setSampled(true);
            const fresh = (update.sessionResults ?? []).slice().reverse();
            if (fresh.length > 0) setEvents((old) => [...fresh, ...old].slice(0, TAIL_KEPT));
          } else if (e.event === "error") setState({ status: "ended", error: JSON.parse(e.data) as { code: string; message: string } });
        },
        ac.signal,
      )
      .then(() => !ac.signal.aborted && setState((s) => (s.status === "ended" ? s : { status: "ended" })))
      .catch((err: unknown) => {
        if (ac.signal.aborted || (err as Error).name === "AbortError") return;
        const failed = err as { code?: string; message?: string };
        setState({ status: "ended", error: { code: failed.code ?? "UNREACHABLE", message: failed.message ?? "The stream could not be opened." } });
      });
    return () => ac.abort();
    // The session is asked for again when what it is for (the key) changes.
  }, [on, key]);
  const start = useCallback(() => {
    setSampled(false);
    setState({ status: "opening" });
    setOn(true);
  }, []);
  const stop = useCallback(() => {
    setOn(false);
    setState({ status: "off" });
  }, []);
  const clear = useCallback(() => setEvents([]), []);
  return { on, events, state, sampled, start, stop, clear };
}
