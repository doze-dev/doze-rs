// The Live tail as the page uses it: the newest calls (`GET /api/v1/calls`), then the stream (`GET /api/v1/tail/live`) from
// where that page ended, for as long as the page is on screen. A stream that ends or fails is opened again by itself, from
// the last call seen (`Last-Event-ID`), so no call is shown twice and none between is lost while the record still holds it.
// Nothing here runs while Live is not on screen: the stream is closed when the page is left.
import { useCallback, useEffect, useReducer, useState } from "react";
import { client, PATHS } from "../../api/client";
import type { CallsPage } from "../../api/types/CallsPage";
import type { TailReady } from "../../api/types/TailReady";
import { EMPTY, reduce, retryDelayMs, type Tape } from "./tape";

/** What the connection is doing. `waiting`: it failed or ended, and is tried again in a moment (`error` says what happened). */
export type Conn = { state: "connecting" } | { state: "live" } | { state: "waiting"; error: unknown };

/** The stream closed without an error: doze-aws stopped, or something between closed it. */
export class StreamEnded extends Error {
  constructor() {
    super("The live stream ended. doze-aws may have stopped.");
    this.name = "StreamEnded";
  }
}

function parse<T>(data: string): T | null {
  try {
    return JSON.parse(data) as T;
  } catch {
    return null;
  }
}

/** Resolves after `ms`, or at once when `signal` aborts. */
function pause(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener("abort", () => (clearTimeout(timer), resolve()), { once: true });
  });
}

export function useTail(): { tape: Tape; conn: Conn; pause: () => void; resume: () => void; retry: () => void } {
  const [tape, dispatch] = useReducer(reduce, EMPTY);
  const [conn, setConn] = useState<Conn>({ state: "connecting" });
  // Bumped by Try again: the loop below starts over at once.
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const ac = new AbortController();
    const { signal } = ac;
    let cursor: string | null = null;
    void (async () => {
      for (let failures = 0; !signal.aborted; ) {
        let failure: unknown;
        try {
          setConn({ state: "connecting" });
          if (cursor === null) {
            // The whole record (the server clamps the limit to its capacity).
            const page = await client.api<CallsPage>(`${PATHS.calls}?limit=100000`, {}, signal);
            cursor = page.cursor;
            dispatch({ type: "loaded", page });
          }
          await client.streamEvents(
            PATHS.tail,
            (e) => {
              if (e.event === "ready") {
                const ready = parse<TailReady>(e.data);
                if (!ready) return;
                failures = 0;
                cursor = ready.cursor;
                setConn({ state: "live" });
                dispatch({ type: "ready", ready });
              } else if (e.event === "calls") {
                const page = parse<CallsPage>(e.data);
                if (!page) return;
                cursor = page.cursor;
                dispatch({ type: "calls", page });
              }
            },
            signal,
            cursor,
          );
          failure = new StreamEnded();
        } catch (e) {
          failure = e;
        }
        if (signal.aborted) return;
        failures += 1;
        setConn({ state: "waiting", error: failure });
        await pause(retryDelayMs(failures), signal);
      }
    })();
    return () => ac.abort();
  }, [attempt]);

  return {
    tape,
    conn,
    pause: useCallback(() => dispatch({ type: "pause" }), []),
    resume: useCallback(() => dispatch({ type: "resume" }), []),
    retry: useCallback(() => setAttempt((n) => n + 1), []),
  };
}
