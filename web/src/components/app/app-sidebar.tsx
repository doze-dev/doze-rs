import { Link, useRouterState } from "@tanstack/react-router";
import { useShell } from "@/api/queries";
import { Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarHeader, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarRail } from "@/components/ui/sidebar";
import { number } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Logo, Wordmark } from "./brand";
import { isActive, NAV } from "./nav";

export function AppSidebar() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const shell = useShell();
  return (
    <Sidebar collapsible="icon" data-testid="sidebar">
      <SidebarHeader className="h-[52px] justify-center border-b border-sidebar-border px-4 group-data-[collapsible=icon]:px-2">
        <Link to="/" className="group/brand flex items-center gap-2.5 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/50" aria-label="doze Kafka, Home">
          <Logo size={32} swing />
          <Wordmark className="text-[20px] leading-none group-data-[collapsible=icon]:hidden" />
        </Link>
      </SidebarHeader>
      <SidebarContent className="pt-3">
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu className="gap-0.5">
              {NAV.map((item) => {
                const active = isActive(item, path);
                const raw = item.count && shell.data ? shell.data[item.count] : undefined;
                // The registry's count is null when there is none: the item stays, and says "off" (Sc-14); its page says why.
                const count = raw === null ? "off" : raw === undefined ? undefined : number(raw);
                return (
                  <SidebarMenuItem key={item.to}>
                    <SidebarMenuButton asChild isActive={active} tooltip={item.label} className="h-9 text-[14px] data-[active=true]:font-medium [&>svg]:text-muted-foreground data-[active=true]:[&>svg]:text-link">
                      <Link to={item.to} aria-current={active ? "page" : undefined}>
                        <item.icon />
                        <span>{item.label}</span>
                      </Link>
                    </SidebarMenuButton>
                    {count !== undefined && (
                      <SidebarMenuBadge className="font-normal text-muted-foreground tabular-nums" data-testid={`count-${item.count}`}>
                        {count}
                      </SidebarMenuBadge>
                    )}
                    {/* The disk needs someone (the last look said so): a dot of its tone, its words on the page and in the top bar. */}
                    {item.disk && shell.data?.disk && (
                      <SidebarMenuBadge data-testid="disk-dot" title={shell.data.disk.headline}>
                        <span aria-hidden className={cn("size-2 rounded-full", shell.data.disk.tone === "bad" ? "bg-bad" : "bg-warn")} />
                        <span className="sr-only">{shell.data.disk.headline}</span>
                      </SidebarMenuBadge>
                    )}
                  </SidebarMenuItem>
                );
              })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarRail />
    </Sidebar>
  );
}
