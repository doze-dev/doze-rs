// The registry of the resource pages the console's app draws, by the descriptor's `deep` ids
// (crates/aws-console/src/resources/*.rs). A service registers its own in `../<service>/index.ts`; `./pages.tsx` imports those
// files and draws what they registered. A tab the server names that nothing registered is shown as not drawn yet, never hidden.
import type { LazyExoticComponent, ReactNode } from "react";
import type { DeepTab } from "../../../api/types/DeepTab";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";

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

export const pageOf = (service: string, id: string): TabPage | undefined => PAGES.get(`${service}/${id}`);
