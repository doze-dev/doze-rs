// The registry of the resource pages the console's app draws, by the descriptor's `deep` ids
// (crates/aws-console/src/resources/*.rs). A service registers its own in `../<service>/index.ts`; `./pages.tsx` imports those
// files and draws what they registered. A tab the server names that nothing registered is shown as not drawn yet, never hidden.
import type { LazyExoticComponent, ReactNode } from "react";
import type { DeepTab } from "../../../api/types/DeepTab";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { OwnSearch } from "../search";

export type DeepProps = { d: ResourceDetail; tab: DeepTab };

/** One page of a resource: a tab of its detail page. */
export type TabPage = {
  page: LazyExoticComponent<(p: DeepProps) => ReactNode>;
  /** Shown only when this holds of the resource (a redrive, on a queue that is some queue's dead-letter queue). */
  when?: (d: ResourceDetail) => boolean;
};

/** What a resource's details open with, for the kinds that have something to say first. */
export type Lead = LazyExoticComponent<(p: { d: ResourceDetail }) => ReactNode>;

const PAGES = new Map<string, TabPage>();
/** By `service/kind`. A plain object: a page picks its lead by indexing it, which a component picked by a call is not. */
export const LEADS: Record<string, Lead> = {};

/** Registers a service's tabs, by the `id` of the descriptor's `deep` entry. */
export function register(service: string, tabs: Record<string, TabPage>): void {
  for (const [id, tab] of Object.entries(tabs)) PAGES.set(`${service}/${id}`, tab);
}

/** Registers what the details of one kind (`stacks`) of a service open with. */
export function registerLead(service: string, kind: string, lead: Lead): void {
  LEADS[`${service}/${kind}`] = lead;
}

/** What a kind says in place of "could not be read" when the resource is not there at its name: where the console can tell more. */
export type Gone = LazyExoticComponent<(p: { name: string }) => ReactNode>;
/** By `service/kind`. */
export const GONE: Record<string, Gone> = {};

/** Registers the page of a name that is gone, for one kind (`stacks`) of a service. */
export function registerGone(service: string, kind: string, gone: Gone): void {
  GONE[`${service}/${kind}`] = gone;
}

export const pageOf = (service: string, id: string): TabPage | undefined => PAGES.get(`${service}/${id}`);

/** The pages a service draws itself in place of the generic ones: a kind's list, one resource's page, and the full-page forms and
 * results of `?do=<id>` (a create, a delete, an upload's status). A kind that registers none gets the generic pages. */
export type OwnProps = { service: string; kind: string; name: string | null; search: OwnSearch & { tab?: string; p?: string } };
export type OwnPage = LazyExoticComponent<(p: OwnProps) => ReactNode>;
export type Own = { list?: OwnPage; one?: OwnPage; actions?: Record<string, OwnPage> };
const OWN = new Map<string, Own>();

/** Registers the pages a service draws itself for one kind (`buckets`). */
export function registerOwn(service: string, kind: string, own: Own): void {
  OWN.set(`${service}/${kind}`, own);
}

export const ownOf = (service: string, kind: string): Own | undefined => OWN.get(`${service}/${kind}`);
