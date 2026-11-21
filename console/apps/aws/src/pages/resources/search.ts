// The Resources address's search, a module of its own so the router does not pull the space's pages in.

/** `?m=QueueUrl`: the input member a call from Live named its resource by, which says its kind. `?all=yes`: a list shows the
 * rows its kind sets aside too (AWS managed keys). */
export function resourceSearch(s: Record<string, unknown>): { m?: string; all?: "yes"; do?: string; name?: string } {
  const out: { m?: string; all?: "yes"; do?: string; name?: string } = {};
  if (typeof s.m === "string" && s.m) out.m = s.m;
  if (s.all === "yes") out.all = "yes";
  Object.assign(out, ownSearch(s));
  return out;
}

/** The addresses of a page of a service's own (`registerOwn`): `?do=create` is a full-page form or a result of the list; `name`
 * is what it acts on (a bucket); `key`, `ver`, `sel` and `dialog` say more (an object, its version, what was selected, the
 * dialog open over a page); `versions=yes` lists a bucket's versions. */
export type OwnSearch = { do?: string; name?: string; key?: string; ver?: string; sel?: string; dialog?: string; versions?: "yes"; mode?: string };
const WORD = /^[a-z-]{1,40}$/;
export function ownSearch(s: Record<string, unknown>): OwnSearch {
  const out: OwnSearch = {};
  const text = (k: "name" | "key" | "ver" | "sel", max: number) => {
    const v = s[k];
    if (typeof v === "string" && v.length > 0 && v.length <= max) out[k] = v;
  };
  const word = (k: "do" | "dialog" | "mode") => {
    const v = s[k];
    if (typeof v === "string" && WORD.test(v)) out[k] = v;
  };
  word("do");
  word("dialog");
  word("mode");
  text("name", 1024);
  text("key", 1024);
  text("ver", 512);
  text("sel", 16384);
  if (s.versions === "yes") out.versions = "yes";
  return out;
}

/** `?tab=objects`: one of the resource's own pages, beside its details (none: the details). `?p=banners/`: where in it the page
 * is (a bucket's folder). `?group=`: the log group of a stream opened in the stream viewer (`/resources/logs/streams/<stream>`). */
export function oneSearch(s: Record<string, unknown>): { tab?: string; p?: string; group?: string } & OwnSearch {
  const out: { tab?: string; p?: string; group?: string } & OwnSearch = ownSearch(s);
  if (typeof s.tab === "string" && /^[a-z-]{1,40}$/.test(s.tab)) out.tab = s.tab;
  if (typeof s.p === "string" && s.p.length <= 1024) out.p = s.p;
  if (typeof s.group === "string" && s.group.length <= 512) out.group = s.group;
  return out;
}
