import type { Crumb } from "@doze/ui/components/app/page-header";
import { useCatalogue } from "../api/resources";

/** The breadcrumb a service's pages begin with: `Resources`, the group it is listed under (`Observability`), the service (`CloudWatch Logs`).
 * Read from the catalogue, which makes no call and is kept, so a page that has read nothing else still knows where it is. */
export function useServiceCrumbs(service: string): Crumb[] {
  const s = useCatalogue().data?.services.find((x) => x.name === service);
  return [{ label: "Resources", to: "/resources" }, ...(s ? [{ label: s.group, to: "/resources" }, { label: s.title, to: `/resources/${service}` }] : [])];
}

const norm = (s: string) => s.trim().toLowerCase();

/** The crumb that is the page itself: one that goes nowhere (`to` empty or `#`), or one that says the page's words. */
const isHere = (c: Crumb, here: string) => c.to === "" || c.to === "#" || norm(c.label) === here || norm(c.label).startsWith(here) || here.startsWith(norm(c.label));

/** The trail with the page itself as its last crumb. A trail that already ends on the page (a crumb with no destination, the
 * same words, or the page's words with more after them: `Create data stream: Provisioned`) is kept as it is; one that ends
 * on a parent (`Queues`, `Keys`) gets the page added, so a page's breadcrumb never stops one level short. `label` is the
 * crumb's words when they are not the page's title (the board's `Edit` for `Edit pick-lists`). */
export function withCurrent(crumbs: Crumb[], title: string, label?: string): Crumb[] {
  const here = norm(label ?? title);
  const last = crumbs[crumbs.length - 1];
  if (last && isHere(last, here)) return crumbs;
  return [...crumbs, { label: label ?? title, to: "" }];
}

/** Where a resource's own pages start: Resources, its group, its service, its kind. */
export function kindCrumbs(d: { group: string; service: string; service_title: string; kind: string; kind_title: string }): Crumb[] {
  return [{ label: "Resources", to: "/resources" }, { label: d.group, to: `/resources/${d.service}` }, { label: d.service_title, to: `/resources/${d.service}` }, { label: d.kind_title, to: `/resources/${d.service}/${d.kind}` }];
}

/** A resource's pages: the kind's trail, the resource (a link to its page, or the page itself when nothing follows), then
 * what follows it (a tab, a form), the last being the page on screen. */
export function resourceCrumbs(d: { name: string; /** What its address says, when that is not its name (a KMS key's id). */ key?: string; group: string; service: string; service_title: string; kind: string; kind_title: string }, ...rest: Crumb[]): Crumb[] {
  const self = { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.key ?? d.name)}` };
  return [...kindCrumbs(d), self, ...rest];
}
