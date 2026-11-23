import { Link, useRouterState } from "@tanstack/react-router";
import { PinIcon, PinOffIcon } from "lucide-react";
import { createElement, useEffect } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarMenu, SidebarMenuButton, SidebarMenuItem } from "@doze/ui/components/ui/sidebar";
import { iconOfKind } from "../pages/resources/model";
import { clearAbsent, labelOf, MOST_RECENT, type Place, pin, placeOf, same, useAbsent, usePinned, useVisited, visit } from "./pins";

/** A page whose address is an ARN (an execution's) is named by the end of it in the rail, the name the page says: the whole ARN is its title. */
export const shownName = (name: string): string => (name.startsWith("arn:") ? (name.split(":").pop() ?? name) : name);

const params = (p: Place) => ({ service: p.service, kind: p.kind, name: p.name });

function Group({ label, places, testId }: { label: string; places: Place[]; testId: string }) {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const here = placeOf(path);
  if (places.length === 0) return null;
  return (
    <SidebarGroup className="pt-2 group-data-[collapsible=icon]:hidden" data-testid={testId}>
      <SidebarGroupLabel className="text-[11px] tracking-[0.1em] uppercase">{label}</SidebarGroupLabel>
      <SidebarGroupContent>
        <SidebarMenu className="gap-0.5">
          {places.map((p) => (
            <SidebarMenuItem key={`${p.service}/${p.kind}/${p.name}`}>
              <SidebarMenuButton asChild isActive={here !== null && same(here, p)} className="h-8 gap-2.5 text-[13px]">
                <Link to="/resources/$service/$kind/$name" params={params(p)} search={{}} title={p.name.startsWith("arn:") ? p.name : `${p.service} · ${p.kind}`} data-testid="rail-place">
                  {createElement(iconOfKind(p.service, p.kind), { "aria-hidden": true, className: "size-4 text-primary" })}
                  <span className="truncate font-mono text-[12.5px]">{shownName(labelOf(p))}</span>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          ))}
        </SidebarMenu>
      </SidebarGroupContent>
    </SidebarGroup>
  );
}

/** Pinned and Recent in the rail: resources, not services. A page of a resource is remembered as opened when it is on screen. */
export function RailPlaces() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const pinned = usePinned();
  const visited = useVisited();
  useEffect(() => {
    const p = placeOf(path);
    if (p) visit(p);
    return clearAbsent;
  }, [path]);
  return (
    <>
      <Group label="Pinned" places={pinned} testId="rail-pinned" />
      <Group label="Recent" places={visited.filter((v) => !pinned.some((p) => same(p, v))).slice(0, MOST_RECENT)} testId="rail-recent" />
    </>
  );
}

/** Pin or unpin the resource whose page is open: in the top bar, only on such a page. */
export function PinButton() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const pinned = usePinned();
  const here = placeOf(path);
  const absent = useAbsent(here);
  if (!here || absent) return null;
  const on = pinned.some((p) => same(p, here));
  return (
    <Button variant="outline" size="sm" className="hidden h-8 gap-1.5 rounded-full bg-card md:inline-flex" onClick={() => pin(here)} aria-pressed={on} data-testid="pin-toggle" title={on ? `Take ${here.name} out of the rail` : `Keep ${here.name} in the rail`}>
      {on ? <PinOffIcon /> : <PinIcon />}
      {on ? "Unpin" : "Pin"}
    </Button>
  );
}
