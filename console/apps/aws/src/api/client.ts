// The console's client: one `createApi` of @doze/ui (every request and the event stream carry `X-Console: 1`, the proof the
// server's API guard asks for), with doze-aws's own words for a request that got no answer.
import { createApi } from "@doze/ui/lib/api";

export const UNREACHABLE = "doze-aws did not answer. Is it still running?";

export const client = createApi({ unreachable: UNREACHABLE });

/** The addresses the API has today (crates/aws-console/src/api.rs). */
export const PATHS = {
  about: "/api/v1/about",
  services: "/api/v1/services",
  calls: "/api/v1/calls",
  tail: "/api/v1/tail/live",
  flows: "/api/v1/flows",
  copy: (seq: number) => `/api/v1/calls/${seq}/copy`,
} as const;
