import { useCallback } from "react";
import type { Crumb } from "@doze/ui/components/app/page-header";
import { useCatalogue } from "../api/resources";
import { ACCESS_SERVICES } from "../nav";

/** What a service's pages begin with: `Resources`, or `Access` for IAM and STS (the rail marks Access on their pages), then the service
 * (`CloudWatch Logs`). Its group (`Observability`) is put between the two by the shell (`withGroup`): a link to that heading of the index. */
export function serviceTrail(service: string, title?: string): Crumb[] {
  const space = ACCESS_SERVICES.includes(service) ? { label: "Access", to: "/access" } : { label: "Resources", to: "/resources" };
  return [space, ...(title ? [{ label: title, to: `/resources/${service}` }] : [])];
}

/** The breadcrumb a service's pages begin with (see {@link serviceTrail}).
 * Read from the catalogue, which makes no call and is kept, so a page that has read nothing else still knows where it is. */
export function useServiceCrumbs(service: string): Crumb[] {
  const s = useCatalogue().data?.services.find((x) => x.name === service);
  return serviceTrail(service, s?.title);
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

/** Where a resource's own pages start: Resources (Access for IAM and STS), its service, its kind. */
export function kindCrumbs(d: { group: string; service: string; service_title: string; kind: string; kind_title: string }): Crumb[] {
  return [...serviceTrail(d.service, d.service_title), { label: d.kind_title, to: `/resources/${d.service}/${d.kind}` }];
}

/** A resource's pages: the kind's trail, the resource (a link to its page, or the page itself when nothing follows), then
 * what follows it (a tab, a form), the last being the page on screen. */
export function resourceCrumbs(d: { name: string; /** What its address says, when that is not its name (a KMS key's id). */ key?: string; group: string; service: string; service_title: string; kind: string; kind_title: string }, ...rest: Crumb[]): Crumb[] {
  const self = { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.key ?? d.name)}` };
  return [...kindCrumbs(d), self, ...rest];
}

/** Where a group's heading is on the Resources index: the index opens at it and marks it. */
export const groupTo = (group: string) => `/resources?group=${encodeURIComponent(group).replace(/%20/g, "+")}`;

/** The board's trail has the service's group between Resources and the service (`Resources > Messaging and events > SQS > Queues`). A page makes
 * `Resources > SQS > ...` and the shell puts the group in (the one place, so no page forgets it): after `Resources`, when the next crumb is a
 * service's page (`/resources/<service>...`) of a service the catalogue gives a group, and not twice. Access (IAM, STS) has no groups. */
export function withGroup(crumbs: Crumb[], groupOf: (service: string) => string | undefined): Crumb[] {
  const [first, next] = crumbs;
  if (first?.to !== "/resources" || !next) return crumbs;
  const service = /^\/resources\/([^/?#]+)/.exec(next.to)?.[1];
  const group = service ? groupOf(decodeURIComponent(service)) : undefined;
  if (!group || crumbs.some((c) => c.label === group)) return crumbs;
  return [first, { label: group, to: groupTo(group) }, ...crumbs.slice(1)];
}

/** The shaping the shell gives every page header: {@link withGroup} with the catalogue's groups (nothing changes until it is read). */
export function useGroupShape(): (crumbs: Crumb[]) => Crumb[] {
  const services = useCatalogue().data?.services;
  return useCallback((crumbs) => withGroup(crumbs, (name) => services?.find((s) => s.name === name)?.group), [services]);
}
