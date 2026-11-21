// The map's flows: `/api/v1/flows` once (the server's counts since doze-aws started), then the calls the stream brings added to
// them here, so the map needs no polling. Asked again when doze-aws restarted (the tape's cursor names another run).
import { useEffect, useMemo, useState } from "react";
import { client, PATHS } from "../../api/client";
import type { CallRecord } from "../../api/types/CallRecord";
import type { FlowsResponse } from "../../api/types/FlowsResponse";
import { type Flows, fromServer, withCalls } from "./flows";

const NONE: Flows = { byKey: new Map(), through: 0 };

export function useFlows(calls: readonly CallRecord[], cursor: string | null): { flows: Flows; failed: unknown } {
  const run = cursor?.split(".")[0] ?? null;
  const [base, setBase] = useState<Flows | null>(null);
  const [failed, setFailed] = useState<unknown>(null);
  useEffect(() => {
    if (run === null) return;
    const ac = new AbortController();
    client
      .api<FlowsResponse>(PATHS.flows, {}, ac.signal)
      .then((r) => {
        setBase(fromServer(r));
        setFailed(null);
      })
      .catch((e: unknown) => {
        if (!ac.signal.aborted) setFailed(e);
      });
    return () => ac.abort();
  }, [run]);
  // Until the counts arrive (or when they could not be had), the map is drawn from the calls on screen alone.
  const flows = useMemo(() => withCalls(base ?? NONE, calls), [base, calls]);
  return { flows, failed };
}
