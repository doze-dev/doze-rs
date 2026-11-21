// The Resources address's search, a module of its own so the router does not pull the space's pages in.

/** `?m=QueueUrl`: the input member a call from Live named its resource by, which says its kind. `?all=yes`: a list shows the
 * rows its kind sets aside too (AWS managed keys). */
export function resourceSearch(s: Record<string, unknown>): { m?: string; all?: "yes"; q?: string; create?: "yes"; gone?: string; from?: string } {
  const out: { m?: string; all?: "yes"; q?: string; create?: "yes"; gone?: string; from?: string } = {};
  if (typeof s.from === "string" && s.from && s.from.length <= 256) out.from = s.from;
  if (typeof s.m === "string" && s.m) out.m = s.m;
  if (s.all === "yes") out.all = "yes";
  if (typeof s.q === "string" && s.q && s.q.length <= 256) out.q = s.q;
  if (s.create === "yes") out.create = "yes";
  if (typeof s.gone === "string" && s.gone && s.gone.length <= 256) out.gone = s.gone;
  return out;
}

/** `?view=edit`: a page of a resource that is not one of its tabs (a queue's Edit, its messages); `?done=created`: the page opens with a
 * banner for what was just done. `?tab=objects`: one of the resource's own pages, beside its details (none: the details). `?p=banners/`: where in it the page
 * is (a bucket's folder). `?group=`: the log group of a stream opened in the stream viewer (`/resources/logs/streams/<stream>`). */
export function oneSearch(s: Record<string, unknown>): { tab?: string; p?: string; group?: string; view?: string; done?: string } {
  const out: { tab?: string; p?: string; group?: string; view?: string; done?: string } = {};
  if (typeof s.view === "string" && /^[a-z-]{1,40}$/.test(s.view)) out.view = s.view;
  if (typeof s.done === "string" && /^[a-z-]{1,40}$/.test(s.done)) out.done = s.done;
  if (typeof s.tab === "string" && /^[a-z-]{1,40}$/.test(s.tab)) out.tab = s.tab;
  if (typeof s.p === "string" && s.p.length <= 1024) out.p = s.p;
  if (typeof s.group === "string" && s.group.length <= 512) out.group = s.group;
  return out;
}
