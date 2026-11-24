import { Link, useRouterState } from "@tanstack/react-router";
import { useMemo } from "react";
import { SidebarMenuSub, SidebarMenuSubButton, SidebarMenuSubItem } from "@doze/ui/components/ui/sidebar";
import { cn } from "@doze/ui/lib/utils";
import { useServices } from "../api/queries";
import { useHome } from "../api/spaces";
import { type AwakeGroup, awakeCount, awakeGroups, STATE_WORDS } from "../pages/home/awake";
import { StateMark } from "../pages/home/state-mark";
import { useNow } from "../pages/card";

/** The group IAM and STS are in: they are under Access, not Resources. */
const ACCESS_GROUP = "Identity and access";

/** Every service of the build in its group, awake or asleep (the answers Home reads; `[]` until they are in). */
function useAwakeGroups(): AwakeGroup[] {
  const services = useServices().data?.services;
  const home = useHome().data;
  const now = useNow(30_000);
  return useMemo<AwakeGroup[]>(() => (services ? awakeGroups(services, home?.activity ?? [], home?.idle_secs ?? 300, now) : []), [services, home, now]);
}

/** What the rail's items count, as the Kafka console's rail counts its topics: the apps that have called (not the calls: this console's
 * own are most of the record, and Live hides them), and the services that are awake (Home says which). `undefined` until the answer is in. */
export function useRailCounts(): { live: number | undefined; awake: { awake: number; served: number } | undefined } {
  const groups = useAwakeGroups();
  const apps = useHome().data?.callers.length;
  return { live: apps, awake: groups.length > 0 ? awakeCount(groups) : undefined };
}

/** The service whose pages are open (`/resources/sqs/queues/orders` is `sqs`), or `null` off the Resources space. */
export function serviceOfPath(path: string): string | null {
  const m = /^\/resources\/([^/]+)/.exec(path);
  return m ? decodeURIComponent(m[1]) : null;
}

/** The services under Resources in the rail: each in its group, a mark for whether it is running (a green dot) or resting (a blue moon), the one
 * whose pages are open marked. The same words and groups as Home's "What is awake", read from the same two answers. */
export function RailServices() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const here = serviceOfPath(path);
  const all = useAwakeGroups();
  const groups = useMemo(() => all.filter((g) => g.name !== ACCESS_GROUP), [all]);
  if (groups.length === 0) return null;
  return (
    <SidebarMenuSub className="mt-0.5 mr-2 mb-1.5 ml-[1.375rem] gap-0 px-1.5" data-testid="rail-services" aria-label="Services">
      {groups.map((g, i) => (
        <li key={g.name} className={cn("list-none", i > 0 && "mt-1 border-t border-sidebar-border/70 pt-1")} data-group={g.name}>
          <ul className="grid gap-px" aria-label={g.name}>
            {g.rows.map((r) => (
              <SidebarMenuSubItem key={r.name}>
                {r.state === "off" ? (
                  <span className="flex h-6 items-center gap-2 px-2 text-[13px] text-muted-foreground/60" title="This run does not serve it (--services)" data-testid="rail-service" data-service={r.name} data-state="off">
                    <StateMark state="off" className="opacity-60" />
                    <span className="truncate">{r.title}</span>
                  </span>
                ) : (
                  <SidebarMenuSubButton asChild isActive={here === r.name} className="h-6 gap-2 text-[13px] data-[active=true]:font-medium">
                    <Link to="/resources/$service" params={{ service: r.name }} aria-current={here === r.name ? "page" : undefined} title={`${r.title}, ${g.name}: ${STATE_WORDS[r.state].toLowerCase()}`} data-testid="rail-service" data-service={r.name} data-state={r.state}>
                      <StateMark state={r.state} />
                      <span className="truncate">{r.title}</span>
                      <span className="sr-only">{STATE_WORDS[r.state]}</span>
                    </Link>
                  </SidebarMenuSubButton>
                )}
              </SidebarMenuSubItem>
            ))}
          </ul>
        </li>
      ))}
    </SidebarMenuSub>
  );
}
