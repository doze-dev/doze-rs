// The Resources address's search, a module of its own so the router does not pull the space's pages in.

/** `?m=QueueUrl`: the input member a call from Live named its resource by, which says its kind. */
export function resourceSearch(s: Record<string, unknown>): { m?: string } {
  return typeof s.m === "string" && s.m ? { m: s.m } : {};
}
