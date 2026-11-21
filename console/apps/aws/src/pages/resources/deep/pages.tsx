// The resource pages the console's app draws, by the descriptor's `deep` ids (crates/aws-console/src/resources/*.rs). A tab the
// server names that this table does not have is shown as not drawn yet, never hidden.
import { Link } from "@tanstack/react-router";
import { lazy, Suspense } from "react";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";
import type { DeepTab } from "../../../api/types/DeepTab";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";

export type DeepProps = { d: ResourceDetail; tab: DeepTab };

type Page = {
  page: React.LazyExoticComponent<(p: DeepProps) => React.ReactNode>;
  /** Shown only when this holds of the resource (a redrive, on a queue that is some queue's dead-letter queue). */
  when?: (d: ResourceDetail) => boolean;
};

const sqs = () => import("./sqs-messages");
const PAGES: Record<string, Page> = {
  "sqs/messages": { page: lazy(() => sqs().then((m) => ({ default: m.QueueMessages }))) },
  "sqs/redrive": {
    page: lazy(() => import("./sqs-redrive").then((m) => ({ default: m.QueueRedrive }))),
    when: (d) => d.subs.some((s) => s.title.includes("dead-letter queue") && s.rows.length > 0),
  },
  "sqs/settings": { page: lazy(() => import("./sqs-settings").then((m) => ({ default: m.QueueSettings }))) },
  "s3/objects": { page: lazy(() => import("./s3-objects").then((m) => ({ default: m.BucketObjects }))) },
  "dynamodb/items": { page: lazy(() => import("./ddb-items").then((m) => ({ default: m.TableItems }))) },
  "lambda/test": { page: lazy(() => import("./lambda/test-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.FunctionTest name={d.name} /> }))) },
  "lambda/configuration": { page: lazy(() => import("./lambda/config-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.FunctionConfig name={d.name} /> }))) },
  "lambda/versions": { page: lazy(() => import("./lambda/lists").then((m) => ({ default: ({ d }: DeepProps) => <m.FunctionVersions name={d.name} /> }))) },
  "lambda/invocations": { page: lazy(() => import("./lambda/lists").then((m) => ({ default: ({ d }: DeepProps) => <m.FunctionInvocations d={d} /> }))) },
  "stepfunctions/executions": { page: lazy(() => import("./sfn/executions-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.MachineExecutions arn={d.id} name={d.name} /> }))) },
  "logs/streams": { page: lazy(() => import("./logs/streams-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.GroupStreams group={d.name} /> }))) },
  "logs/search": { page: lazy(() => import("./logs/search-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.GroupSearch group={d.name} /> }))) },
  "logs/tail": { page: lazy(() => import("./logs/tail-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.GroupTail group={d.name} /> }))) },
  "cloudformation/events": { page: lazy(() => import("./cfn/stack-tabs").then((m) => ({ default: ({ d }: DeepProps) => <m.StackEvents name={d.name} /> }))) },
  "cloudformation/template": { page: lazy(() => import("./cfn/stack-tabs").then((m) => ({ default: ({ d }: DeepProps) => <m.StackTemplate name={d.name} /> }))) },
  // Never listed as a tab (`when` is never true): the descriptor names RestoreSecret on this kind so that the banner on the details may make it.
  "secretsmanager/restore": { page: lazy(() => import("./secret-marked").then((m) => ({ default: m.SecretMarked }))), when: () => false },
  // Never listed as a tab: the descriptor names ListStacks (deleted stacks only) so that the page of a name that is gone may read it.
  "cloudformation/deleted": { page: lazy(() => import("./cfn/deleted").then((m) => ({ default: ({ d }: DeepProps) => <m.DeletedStack name={d.name} /> }))), when: () => false },
  "cloudformation/changesets": { page: lazy(() => import("./cfn/stack-tabs").then((m) => ({ default: ({ d }: DeepProps) => <m.StackChangeSets name={d.name} /> }))) },
};

/** What a resource's details open with, for the kinds that have something to say first (a stack's status, as a sentence). */
const LEADS: Record<string, React.LazyExoticComponent<(p: { d: ResourceDetail }) => React.ReactNode>> = {
  "cloudformation/stacks": lazy(() => import("./cfn/stack-tabs").then((m) => ({ default: m.StackSentence }))),
  "secretsmanager/secrets": lazy(() => import("./secret-marked").then((m) => ({ default: m.SecretMarked }))),
};

/** What a kind says in place of "could not be read" when the resource is not there at its name: where the console can tell more. */
const GONE: Record<string, React.LazyExoticComponent<(p: { name: string }) => React.ReactNode>> = {
  "cloudformation/stacks": lazy(() => import("./cfn/deleted").then((m) => ({ default: m.DeletedStack }))),
};

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
  return d.deep.filter((t) => PAGES[`${d.service}/${t.id}`]?.when?.(d) ?? true);
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
  const entry = PAGES[`${d.service}/${id}`];
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
