// The Resources address's search, a module of its own so the router does not pull the space's pages in.

export type ResourceSearch = { m?: string; all?: "yes"; tab?: string; group?: string };

/** `?m=QueueUrl`: the input member a call from Live named its resource by, which says its kind. `?all=yes`: a list shows the
 * rows its kind sets aside too (AWS managed keys). `?tab=`: a deep page's tab. `?group=`: the log group of a stream opened in
 * the stream viewer (`/resources/logs/streams/<stream>`). */
export function resourceSearch(s: Record<string, unknown>): ResourceSearch {
  const out: ResourceSearch = {};
  if (typeof s.m === "string" && s.m) out.m = s.m;
  if (s.all === "yes") out.all = "yes";
  if (typeof s.tab === "string" && s.tab) out.tab = s.tab;
  if (typeof s.group === "string" && s.group) out.group = s.group;
  return out;
}
