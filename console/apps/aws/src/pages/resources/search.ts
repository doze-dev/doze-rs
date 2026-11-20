// The Resources address's search, a module of its own so the router does not pull the space's pages in.

/** `?m=QueueUrl`: the input member a call from Live named its resource by, which says its kind. `?all=yes`: a list shows the
 * rows its kind sets aside too (AWS managed keys). */
export function resourceSearch(s: Record<string, unknown>): { m?: string; all?: "yes" } {
  const out: { m?: string; all?: "yes" } = {};
  if (typeof s.m === "string" && s.m) out.m = s.m;
  if (s.all === "yes") out.all = "yes";
  return out;
}
