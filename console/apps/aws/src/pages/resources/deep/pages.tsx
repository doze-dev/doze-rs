// The resource pages the console's app draws, by the descriptor's `deep` ids (crates/aws-console/src/resources/*.rs). Each
// service registers its own in `../<service>/index.ts` (see ./registry.ts); this file draws what they registered. A tab the
// server names that nothing registered is shown as not drawn yet, never hidden.
import { Link } from "@tanstack/react-router";
import { Suspense } from "react";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";
import type { DeepTab } from "../../../api/types/DeepTab";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { GONE, LEADS, pageOf } from "./registry";
import "../cloudformation";
import "../dynamodb";
import "../iam";
import "../kinesis";
import "../lambda";
import "../logs";
import "../s3";
import "../secretsmanager";
import "../sns";
import "../sqs";
import "../stepfunctions";

export type { DeepProps } from "./registry";

/** Whether the kind has a page for a name that is gone, and the refusal says it is (CloudFormation: ValidationError, "does not exist"). */
export const hasGonePage = (service: string, kind: string, message: string): boolean => `${service}/${kind}` in GONE && /does not exist/i.test(message);

/** The page of a name that is gone. */
export function GonePage({ service, kind, name }: { service: string; kind: string; name: string }) {
  const Gone = GONE[`${service}/${kind}`];
  if (!Gone) return null;
  return (
    <Suspense fallback={<Skeleton className="h-28 rounded-[14px]" />}>
      <Gone name={name} />
    </Suspense>
  );
}

/** The lead of `d`'s details, if its kind has one. */
export function DetailLead({ d }: { d: ResourceDetail }) {
  const Lead = LEADS[`${d.service}/${d.kind}`];
  if (!Lead) return null;
  return (
    <Suspense fallback={null}>
      <Lead d={d} />
    </Suspense>
  );
}

/** The resource's own pages that apply to it now. */
export function tabsOf(d: ResourceDetail): DeepTab[] {
  return d.deep.filter((t) => pageOf(d.service, t.id)?.when?.(d) ?? true);
}

/** The strip of tabs over a resource's page: its details, then its own pages. Each is an address (`?tab=`). */
export function DeepTabs({ d, name, tab }: { d: ResourceDetail; name: string; tab: string | undefined }) {
  const tabs = tabsOf(d);
  if (tabs.length === 0) return null;
  const all = [{ id: undefined, title: "Details" }, ...tabs.map((t) => ({ id: t.id as string | undefined, title: t.title }))];
  return (
    <nav className="mb-8 flex gap-8 overflow-x-auto border-b" aria-label={`${d.one} pages`} data-testid="deep-tabs">
      {all.map((t) => {
        const on = t.id === tab;
        return (
          <Link
            key={t.title}
            to="/resources/$service/$kind/$name"
            params={{ service: d.service, kind: d.kind, name }}
            search={t.id ? { tab: t.id } : {}}
            aria-current={on ? "page" : undefined}
            className={cn("-mb-px border-b-2 px-0.5 pt-3 pb-3 text-[14px] whitespace-nowrap transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", on ? "border-primary font-medium text-bright" : "border-transparent text-muted-foreground hover:text-foreground")}
            data-testid={`tab-${t.id ?? "details"}`}
          >
            {t.title}
          </Link>
        );
      })}
    </nav>
  );
}

/** The page of tab `id`, or the words that say it is not drawn yet. */
export function DeepPage({ d, id }: { d: ResourceDetail; id: string }) {
  const tab = tabsOf(d).find((t) => t.id === id);
  const entry = pageOf(d.service, id);
  if (!tab || !entry) {
    return (
      <p className="rounded-[12px] border border-dashed px-5 py-4 text-[13.5px] text-muted-foreground" data-testid="deep-missing">
        This {d.one} has no page called {id} in this console.
      </p>
    );
  }
  const Page = entry.page;
  return (
    <Suspense fallback={<Skeleton className="h-80 rounded-[14px]" />}>
      <div className="animate-in fade-in-0 duration-200" data-testid={`deep-${id}`}>
        <Page d={d} tab={tab} />
      </div>
    </Suspense>
  );
}
