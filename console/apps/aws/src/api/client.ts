// The console's client: one `createApi` of @doze/ui (every request and the event stream carry `X-Console: 1`, the proof the
// server's API guard asks for), with doze-aws's own words for a request that got no answer.
import { createApi } from "@doze/ui/lib/api";
import { deletedBy, forget } from "../shell/pins";
import { regionHeader } from "./region";

export const UNREACHABLE = "doze-aws did not answer. Is it still running?";

export const client = createApi({
  unreachable: UNREACHABLE,
  // A resource that was deleted leaves the rail's Pinned and Recent: it is not somewhere to go back to.
  fetch: (async (url: RequestInfo | URL, init?: RequestInit) => {
    // The region the console looks at rides on every request (none chosen: the server's default).
    const res = await fetch(url, { ...init, headers: { ...Object.fromEntries(new Headers(init?.headers)), ...regionHeader() } });
    if (res.ok && init?.method === "DELETE") {
      const gone = deletedBy(String(url));
      if (gone) forget(gone);
    }
    return res;
  }) as typeof fetch,
});

/** The addresses the API has today (crates/aws-console/src/api.rs). */
export const PATHS = {
  about: "/api/v1/about",
  services: "/api/v1/services",
  calls: "/api/v1/calls",
  tail: "/api/v1/tail/live",
  flows: "/api/v1/flows",
  wiring: "/api/v1/live/wiring",
  copy: (seq: number) => `/api/v1/calls/${seq}/copy`,
} as const;
