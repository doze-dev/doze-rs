// A log group's Live Tail (crates/aws-console/src/api/deep.rs, `GET …/groups/<group>/tail`): the service's own stream, open
// while the page asks for it, carried as server-sent events. The group is the address's; the page asks only for streams and a
// pattern. Nothing here polls.
import { useCallback, useEffect, useRef, useState } from "react";
import { client } from "../../../api/client";
import { RESOURCE_PATHS } from "../../../api/resources";

const enc = encodeURIComponent;

/** The address of a Live Tail of `group`. */
export const tailPath = (group: string, streams: string[], pattern: string) => {
  const q = [...streams.map((s) => `stream=${enc(s)}`), pattern.trim() ? `pattern=${enc(pattern)}` : ""].filter(Boolean).join("&");
  return `${RESOURCE_PATHS.one("logs", "groups", group)}/tail${q ? `?${q}` : ""}`;
};
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
        tailPath(asked.current.group, asked.current.streams, asked.current.pattern),
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
