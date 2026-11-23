import { Link } from "@tanstack/react-router";
import { createElement, useCallback } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";
import { number } from "@doze/ui/lib/format";
import { useResourcesIndex } from "../../api/resources";
import type { KindCount } from "../../api/types/KindCount";
import type { ResourceService } from "../../api/types/ResourceService";
import { iconOf } from "./model";

import { HINT } from "../../kit";
export { HINT };

/** The group IAM and STS are in: drawn by Access, not by this index. */
const ACCESS_GROUP = "Identity and access";

/** `3 queues`, `1 topic`, `at least 1,000 keys`; a kind not counted says why in a word. */
export function countWords(k: KindCount): string {
  if (k.count === null) return k.error ? `${k.title}: not read` : k.title;
  const noun = k.count === 1 ? k.one : k.title.toLowerCase();
  // What a kind sets aside is said apart: `1 key + 2 AWS managed`.
  const others = k.others && k.others.count > 0 ? ` + ${number(k.others.count)} ${k.others.what.replace(/ \S+$/, "")}` : "";
  return `${k.more ? "at least " : ""}${number(k.count)} ${noun}${others}`;
}

function ServiceCard({ s }: { s: ResourceService }) {
  const first = s.kinds[0];
  const live = s.described && s.served && first;
  const body = (
    <div className="flex items-start gap-3.5">
      <span className="inline-flex size-9 flex-none items-center justify-center rounded-[var(--radius-field)] border bg-muted">
        {createElement(iconOf(s.name), { "aria-hidden": true, className: cn("size-[18px]", live ? "text-primary" : "text-muted-foreground") })}
      </span>
      <div className="min-w-0">
        <div className="text-[16px] font-semibold text-bright">{s.title}</div>
        <div className="mt-0.5 text-[13px] text-muted-foreground" data-testid={`count-${s.name}`}>
          {!s.described ? (s.kinds.length === 0 && s.note?.startsWith("STS") ? "No resources" : "Not described yet") : !s.served ? "Not served in this run (--services)" : s.kinds.map(countWords).join(" · ")}
        </div>
      </div>
    </div>
  );
  const card = "block rounded-[var(--radius-card)] border bg-card px-[22px] py-5";
  if (!live) {
    return (
      <div className={cn(card, "opacity-75")} title={s.note ?? (s.served ? undefined : "This doze-aws does not serve it: start it with the service in --services.")} data-testid={`service-${s.name}`}>
        {body}
      </div>
    );
  }
  return (
    <Link to="/resources/$service/$second" params={{ service: s.name, second: first.id }} search={{}} className={cn(card, "transition-colors hover:border-input focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none")} data-testid={`service-${s.name}`}>
      {body}
    </Link>
  );
}

/** Resources: every service of the build by what it is for, each with its kinds and how many there are now. A count is a list
 * call made when the page is opened or refreshed (Live shows them as the console's); a service this run does not serve is not
 * woken to be counted. */
export function ResourcesPage() {
  const index = useResourcesIndex();
  const { refetch } = index;
  const refresh = useCallback(() => void refetch(), [refetch]);
  const groups: [string, ResourceService[]][] = [];
  for (const s of index.data?.services ?? []) {
    // IAM and STS are the Access space's (resources-index keeps them under Access only); their pages stay at /resources/iam and /resources/sts.
    if (s.group === ACCESS_GROUP) continue;
    const g = groups.find(([name]) => name === s.group);
    if (g) g[1].push(s);
    else groups.push([s.group, [s]]);
  }
  return (
    <>
      <PageHeader
        title="Resources"
        sub={
          <>
            <span className="text-foreground">Everything you can create and poke, grouped by what it is for.</span> Pick a service to see its resources.
          </>
        }
        refresh={<RefreshControl updatedAt={index.dataUpdatedAt} fetching={index.isFetching} onRefresh={refresh} interval={false} />}
      />
      {index.isError && !index.data ? (
        <ErrorBlock what="The resources could not be counted" error={index.error} hint={HINT} onRetry={refresh} retrying={index.isFetching} testId="resources-error" />
      ) : !index.data ? (
        <div className="grid gap-6 md:grid-cols-2 xl:grid-cols-3" data-testid="resources-loading">
          {[0, 1, 2, 3, 4, 5].map((i) => (
            <Skeleton key={i} className="h-[86px] rounded-[var(--radius-card)]" />
          ))}
        </div>
      ) : (
        <div data-testid="resources-index">
          {groups.map(([group, services]) => (
            <section key={group} className="mb-10" aria-labelledby={`group-${group}`}>
              <h2 id={`group-${group}`} className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
                {group}
              </h2>
              <div className="mt-4 grid gap-6 md:grid-cols-2 xl:grid-cols-3">
                {services.map((s) => (
                  <ServiceCard key={s.name} s={s} />
                ))}
              </div>
            </section>
          ))}
        </div>
      )}
    </>
  );
}
