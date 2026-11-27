// The Resources address's search, a module of its own so the router does not pull the space's pages in.

/** `?m=QueueUrl`: the input member a call from Live named its resource by, which says its kind. `?all=yes`: a list shows the
 * rows its kind sets aside too (AWS managed keys). */
export function resourceSearch(s: Record<string, unknown>): { m?: string; all?: "yes"; q?: string; create?: "yes"; gone?: string; from?: string } & OwnSearch {
  const out: { m?: string; all?: "yes"; q?: string; create?: "yes"; gone?: string; from?: string } & OwnSearch = {};
  if (typeof s.from === "string" && s.from && s.from.length <= 256) out.from = s.from;
  if (typeof s.m === "string" && s.m) out.m = s.m;
  if (s.all === "yes") out.all = "yes";
  if (typeof s.q === "string" && s.q && s.q.length <= 256) out.q = s.q;
  if (s.create === "yes") out.create = "yes";
  if (typeof s.gone === "string" && s.gone && s.gone.length <= 256) out.gone = s.gone;
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

/** `?view=edit`: a page of a resource that is not one of its tabs (a queue's Edit, its messages); `?done=created`: the page opens with a
 * banner for what was just done. `?tab=objects`: one of the resource's own pages, beside its details (none: the details). `?p=banners/`: where in it the page
 * is (a bucket's folder). `?group=`: the log group of a stream opened in the stream viewer (`/resources/logs/streams/<stream>`). */
export function oneSearch(s: Record<string, unknown>): { tab?: string; p?: string; group?: string; view?: string; done?: string } & OwnSearch {
  const out: { tab?: string; p?: string; group?: string; view?: string; done?: string } & OwnSearch = ownSearch(s);
  if (typeof s.view === "string" && /^[a-z-]{1,40}$/.test(s.view)) out.view = s.view;
  if (typeof s.done === "string" && /^[a-z-]{1,40}$/.test(s.done)) out.done = s.done;
  if (typeof s.tab === "string" && /^[a-z-]{1,40}$/.test(s.tab)) out.tab = s.tab;
  if (typeof s.p === "string" && s.p.length <= 1024) out.p = s.p;
  if (typeof s.group === "string" && s.group.length <= 512) out.group = s.group;
  return out;
}

/** `?group=Messaging+and+events`: the Resources index opens at that group's heading and marks it (a breadcrumb's group links here). */
export function indexSearch(s: Record<string, unknown>): { group?: string } {
  return typeof s.group === "string" && s.group && s.group.length <= 100 ? { group: s.group } : {};
}

/** The id of a group's heading on the index: its name as a word (`Messaging and events` is `group-messaging-and-events`), which an id and `aria-labelledby` can hold. */
export const groupId = (group: string) => `group-${group.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")}`;
