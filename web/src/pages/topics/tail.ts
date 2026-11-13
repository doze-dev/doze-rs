// The Messages tail (live-design.md rule 6, CON-809): opt-in, server-paced, coalesced; rows are prepended by stable id, so
// nothing on screen is rewritten; held (behind a "N new" pill) while the person is not at the top of the table, or is
// hovering, selecting or focused in it; on a dropped connection it reconnects with the newest id it has (no duplicates).
import { useCallback, useEffect, useRef, useState } from "react";
import { streamEvents } from "@/api/sse";
import type { RecordRow } from "@/api/types/RecordRow";
import type { TailRows } from "@/api/types/TailRows";

// The speed is the one preference of every tail: `useSpeed` in pages/live/tail.ts, shown by components/app/tail-controls.

export const rowId = (r: { partition: number; offset: number }) => `${r.partition}-${r.offset}`;

/** The most rows the tail keeps on screen, and held. */
export const WINDOW = 200;
export const MOST_HELD = 500;
/** The waits between reconnects, in seconds. */
export const BACKOFF = [1, 2, 4, 8, 15];

export type Conn = { state: "connecting" } | { state: "live" } | { state: "reconnecting" } | { state: "down"; retryAt: number; why: string };

/** Merges rows a look found into what is on screen: new ones first, none twice, at most `most`. */
export function prepend(rows: RecordRow[], onto: RecordRow[], most: number): RecordRow[] {
  const have = new Set(onto.map(rowId));
  const fresh = rows.filter((r) => !have.has(rowId(r)));
  return fresh.length ? [...fresh, ...onto].slice(0, most) : onto;
}

/**
 * Follows `path` (the tail's address without `since`) from `cursor`. `isHeld` says, when rows arrive, whether the person is
 * reading (then they wait behind the pill). `known` are the ids the page already shows.
 */
export function useMessagesTail({ path, cursor, isHeld, known }: { path: string | null; cursor: number; isHeld: () => boolean; known: Set<string> }) {
  const [conn, setConn] = useState<Conn>({ state: "connecting" });
  const [live, setLive] = useState<RecordRow[]>([]);
  const [held, setHeld] = useState<RecordRow[]>([]);
  const [fresh, setFresh] = useState<Set<string>>(() => new Set());
  const start = useRef(cursor);
  const knownRef = useRef(known);
  const heldRef = useRef(isHeld);
  useEffect(() => {
    knownRef.current = known;
    heldRef.current = isHeld;
  });
  // Every id the tail has put on screen or held, so a reconnect's replay never adds one twice.
  const seen = useRef(new Set<string>());
  const heldBuf = useRef<RecordRow[]>([]);

  const mark = useCallback((rows: RecordRow[]) => {
    const ids = new Set(rows.map(rowId));
    setFresh(ids);
    setTimeout(() => setFresh((f) => (f === ids ? new Set() : f)), 1600);
  }, []);

  useEffect(() => {
    if (!path) return;
    const ctrl = new AbortController();
    let last = String(start.current);
    let attempt = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const run = async () => {
      await Promise.resolve();
      if (ctrl.signal.aborted) return;
      setConn(attempt === 0 ? { state: "connecting" } : { state: "reconnecting" });
      let why = "The stream ended.";
      try {
        await streamEvents(
          `${path}&since=${encodeURIComponent(last)}`,
          (e) => {
            if (e.event === "ready") {
              attempt = 0;
              setConn({ state: "live" });
              return;
            }
            if (e.event !== "rows") return;
            if (e.id) last = e.id;
            let body: TailRows;
            try {
              body = JSON.parse(e.data) as TailRows;
            } catch {
              return;
            }
            const rows = body.rows.filter((r) => {
              const id = rowId(r);
              if (seen.current.has(id) || knownRef.current.has(id)) return false;
              seen.current.add(id);
              return true;
            });
            if (!rows.length) return;
            if (heldRef.current()) {
              heldBuf.current = [...rows, ...heldBuf.current].slice(0, MOST_HELD);
              setHeld(heldBuf.current);
            } else {
              const all = [...rows, ...heldBuf.current];
              heldBuf.current = [];
              setHeld([]);
              setLive((l) => [...all, ...l].slice(0, WINDOW));
              mark(all);
            }
          },
          ctrl.signal,
          attempt > 0 ? last : undefined,
        );
      } catch (err) {
        if (ctrl.signal.aborted) return;
        why = (err as Error).message || why;
      }
      if (ctrl.signal.aborted) return;
      const wait = BACKOFF[Math.min(attempt, BACKOFF.length - 1)]!;
      attempt++;
      setConn({ state: "down", retryAt: Date.now() + wait * 1000, why });
      timer = setTimeout(() => void run(), wait * 1000);
    };
    void run();
    return () => {
      ctrl.abort();
      clearTimeout(timer);
    };
  }, [path, mark]);

  /** Puts the held rows on screen (the pill's Show). */
  const show = useCallback(() => {
    const h = heldBuf.current;
    heldBuf.current = [];
    setHeld([]);
    if (h.length) {
      setLive((l) => [...h, ...l].slice(0, WINDOW));
      mark(h);
    }
  }, [mark]);

  return { conn, live, held, fresh, show };
}
